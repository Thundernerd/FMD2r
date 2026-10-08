//! FMD2's `HTTP` object, reproducing baseunits/lua/LuaHTTPSend.pas over an `fmd-http`
//! [`HttpSession`] (FMD2's `THTTPSendThread`).

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use fmd_http::{HttpClient, HttpSession, ModuleHttp, NameValueList, Proxy, ProxyKind};
use mlua::{AnyUserData, Lua, Value, Variadic};

use crate::class::to_bytes;
use crate::{LuaClass, LuaMemoryStream, LuaStrings, StringList};

/// The state behind one Lua `HTTP` object: the session plus the `Headers`, `Cookies` and
/// `Document` objects that Lua reads and writes.
///
/// FMD2 hands Lua the session's own `Headers`, `Cookies` and `Document`
/// (baseunits/lua/LuaHTTPSend.pas:157-159). Here the session keeps its own copies, so every
/// method copies the Lua-side objects into the session before it runs and back afterwards;
/// between method calls only Lua can change them, so both sides always agree.
struct HttpObject {
    session: HttpSession,
    headers: LuaStrings,
    cookies: LuaStrings,
    document: LuaMemoryStream,
}

impl HttpObject {
    /// Runs `f` on the session with the Lua-side objects copied in, then copies them back.
    fn with_session<R>(&mut self, f: impl FnOnce(&mut HttpSession) -> R) -> mlua::Result<R> {
        {
            let headers = borrow(self.headers.list())?;
            copy_into(&headers, self.session.headers_mut());
            let cookies = borrow(self.cookies.list())?;
            copy_into(&cookies, self.session.cookies_mut());
            let document = borrow(self.document.stream())?;
            *self.session.document_mut() = document.bytes().to_vec();
        }
        let result = f(&mut self.session);
        copy_from(self.session.headers(), &mut *borrow(self.headers.list())?);
        copy_from(self.session.cookies(), &mut *borrow(self.cookies.list())?);
        let mut document = borrow(self.document.stream())?;
        // Synapse rewinds `Document` after a response (baseunits/synapse/httpsend.pas:718).
        document
            .load(self.session.document())
            .map_err(mlua::Error::external)?;
        document.set_position(0);
        Ok(result)
    }

    /// Runs a request; an `fmd-http` usage error (a call from inside a tokio runtime) becomes a
    /// Lua error.
    fn request(
        &mut self,
        f: impl FnOnce(&mut HttpSession) -> Result<bool, fmd_http::HttpError>,
    ) -> mlua::Result<bool> {
        self.with_session(f)?.map_err(mlua::Error::external)
    }
}

fn borrow<T>(cell: &RefCell<T>) -> mlua::Result<std::cell::RefMut<'_, T>> {
    cell.try_borrow_mut().map_err(mlua::Error::external)
}

/// Replaces `target`'s lines with `source`'s items. Header and cookie lines are text; bytes
/// that are not UTF-8 are replaced.
fn copy_into(source: &StringList, target: &mut NameValueList) {
    *target.lines_mut() = source
        .items()
        .iter()
        .map(|item| String::from_utf8_lossy(item).into_owned())
        .collect();
}

/// Replaces `target`'s items with `source`'s lines.
fn copy_from(source: &NameValueList, target: &mut StringList) {
    target.clear();
    for line in source.lines() {
        target.add(line.as_bytes());
    }
}

/// A Lua `HTTP` object over one [`HttpSession`] (baseunits/lua/LuaHTTPSend.pas).
pub struct LuaHttp {
    object: Rc<RefCell<HttpObject>>,
}

impl LuaHttp {
    /// Wraps `session`. `Headers` and `Cookies` start as the session's, with FMD2's
    /// separators: `:` for headers, `=` and delimiter `;` for cookies
    /// (baseunits/httpsendthread.pas:503-508).
    pub fn new(session: HttpSession) -> LuaHttp {
        let headers = LuaStrings::new();
        {
            let mut list = headers.list().borrow_mut();
            list.set_name_value_separator(b':');
            copy_from(session.headers(), &mut list);
        }
        let cookies = LuaStrings::new();
        {
            let mut list = cookies.list().borrow_mut();
            list.set_delimiter(b';');
            copy_from(session.cookies(), &mut list);
        }
        let document = LuaMemoryStream::new();
        // A fresh stream; loading a slice into it cannot fail for want of memory in practice,
        // and an empty document is what a fresh session holds anyway.
        let _ = document.stream().borrow_mut().load(session.document());
        LuaHttp {
            object: Rc::new(RefCell::new(HttpObject {
                session,
                headers,
                cookies,
                document,
            })),
        }
    }

    /// Creates the Lua `HTTP` object (`luaHTTPSendThreadAddMetaTable`,
    /// baseunits/lua/LuaHTTPSend.pas:153-165).
    pub fn build(&self, lua: &Lua) -> crate::Result<AnyUserData> {
        let (headers, cookies, document) = {
            let object = self.object.borrow();
            (
                object.headers.build(lua)?,
                object.cookies.build(lua)?,
                object.document.build(lua)?,
            )
        };
        LuaClass::new(self.object.clone())
            // `http_request` (baseunits/lua/LuaHTTPSend.pas:21-25): any method; true when the
            // response body is non-empty (baseunits/httpsendthread.pas:592-719).
            .method(
                "Request",
                |lua, http: &mut HttpObject, (method, url): (Value, Value)| {
                    let (method, url) = (text(lua, method)?, text(lua, url)?);
                    http.request(|s| s.request(&method, &url))
                },
            )
            // `http_get` (baseunits/lua/LuaHTTPSend.pas:27-31): true when the response body
            // is non-empty, whatever the status (baseunits/httpsendthread.pas:718, :725-728).
            .method("GET", |lua, http: &mut HttpObject, url: Value| {
                let url = text(lua, url)?;
                http.request(|s| s.get(&url))
            })
            // `http_post` (baseunits/lua/LuaHTTPSend.pas:33-38): non-empty `data` replaces the
            // document; a `Content-Type` header moves into `MimeType`, and `text/html` is sent
            // as form-urlencoded (baseunits/httpsendthread.pas:730-747).
            .method(
                "POST",
                |lua, http: &mut HttpObject, (url, data): (Value, Value)| {
                    let url = text(lua, url)?;
                    let data = to_bytes(lua, data)?;
                    http.request(|s| s.post(&url, &data))
                },
            )
            // `http_head` (baseunits/lua/LuaHTTPSend.pas:40-44, baseunits/httpsendthread.pas:720-723).
            .method("HEAD", |lua, http: &mut HttpObject, url: Value| {
                let url = text(lua, url)?;
                http.request(|s| s.head(&url))
            })
            // `http_xhr` (baseunits/lua/LuaHTTPSend.pas:46-50): resets stale response headers,
            // adds `X-Requested-With: XMLHttpRequest`, then GETs
            // (baseunits/httpsendthread.pas:749-755).
            .method("XHR", |lua, http: &mut HttpObject, url: Value| {
                let url = text(lua, url)?;
                http.request(|s| s.xhr(&url))
            })
            // `http_reset` (baseunits/lua/LuaHTTPSend.pas:52-56): `ResetBasic` plus browser-like
            // default headers (baseunits/httpsendthread.pas:924-932).
            .method("Reset", |_, http: &mut HttpObject, ()| {
                http.with_session(HttpSession::reset)
            })
            // `http_resetbasic` (baseunits/lua/LuaHTTPSend.pas:58-62): clears the document,
            // headers and cookies; `MimeType` back to `text/html`
            // (baseunits/httpsendthread.pas:934-946).
            .method("ResetBasic", |_, http: &mut HttpObject, ()| {
                http.with_session(HttpSession::reset_basic)
            })
            // `http_clearcookies` (baseunits/lua/LuaHTTPSend.pas:64-68): clears `Cookies` and
            // skips the module's cookie jar for the next request
            // (baseunits/httpsendthread.pas:948-952).
            .method("ClearCookies", |_, http: &mut HttpObject, ()| {
                http.with_session(HttpSession::clear_cookies)
            })
            // `http_clearcookiesstorage` (baseunits/lua/LuaHTTPSend.pas:70-74): clears `Cookies`
            // and the module's cookie jar (baseunits/httpsendthread.pas:954-959).
            .method("ClearCookiesStorage", |_, http: &mut HttpObject, ()| {
                http.with_session(HttpSession::clear_cookies_storage)
            })
            // `http_getcookies` (baseunits/lua/LuaHTTPSend.pas:101-105): `Cookies` joined by
            // `; ` (baseunits/httpsendthread.pas:757-767).
            .method("GetCookies", |lua, http: &mut HttpObject, ()| {
                let cookies = http.with_session(|s| s.get_cookies())?;
                lua.create_string(cookies)
            })
            // `http_addservercookies` (baseunits/lua/LuaHTTPSend.pas:76-83): with exactly two
            // arguments `(url, cookies)`, otherwise `(cookies)` with an empty URL; the
            // `Set-Cookie` lines go into the module's jar, dated now
            // (baseunits/httpsendthread.pas:783-788).
            .method(
                "AddServerCookies",
                |lua, http: &mut HttpObject, args: Variadic<Value>| {
                    let (url, cookies) = match <[Value; 2]>::try_from(args.to_vec()) {
                        Ok([url, cookies]) => (text(lua, url)?, text(lua, cookies)?),
                        Err(args) => {
                            let first = args.into_iter().next().unwrap_or(Value::Nil);
                            (String::new(), text(lua, first)?)
                        }
                    };
                    http.session.add_server_cookies(&url, &cookies);
                    Ok(())
                },
            )
            // `http_parseservercookies` (baseunits/lua/LuaHTTPSend.pas:85-89): stores the
            // `Set-Cookie` lines of `Headers` in the module's jar
            // (baseunits/httpsendthread.pas:481-485).
            .method("ParseServerCookies", |_, http: &mut HttpObject, ()| {
                http.with_session(|s| s.parse_server_cookies())
            })
            // `http_setproxy(type, host, port, user, pass)` (baseunits/lua/LuaHTTPSend.pas:91-96):
            // `HTTP`, `SOCKS4` or `SOCKS5` in any case; any other type means no proxy
            // (baseunits/httpsendthread.pas:838-874).
            .method(
                "SetProxy",
                |lua, http: &mut HttpObject, args: (Value, Value, Value, Value, Value)| {
                    let (kind, host, port, user, pass) = (
                        text(lua, args.0)?,
                        text(lua, args.1)?,
                        text(lua, args.2)?,
                        text(lua, args.3)?,
                        text(lua, args.4)?,
                    );
                    http.session.set_proxy(&kind, &host, &port, &user, &pass);
                    Ok(())
                },
            )
            // `http_threadterminated` (baseunits/lua/LuaHTTPSend.pas:107-111, :145): whether
            // the owner thread was terminated (baseunits/httpsendthread.pas:810-816).
            .read_only_property("Terminated", |_, http: &mut HttpObject| {
                Ok(http.session.terminated())
            })
            // `http_threadlasturl` (baseunits/lua/LuaHTTPSend.pas:113-117, :146): the URL of
            // the last request, after normalisation and redirects
            // (baseunits/httpsendthread.pas:613-616, :657).
            .read_only_property("LastURL", |lua, http: &mut HttpObject| {
                lua.create_string(http.session.last_url())
            })
            // `http_threadresultcode` (baseunits/lua/LuaHTTPSend.pas:119-123, :147): 500 after
            // a transport error (baseunits/synapse/httpsend.pas:436).
            .read_only_property("ResultCode", |_, http: &mut HttpObject| {
                Ok(http.session.result_code())
            })
            // `http_threadresultstring` (baseunits/lua/LuaHTTPSend.pas:125-129, :148).
            .read_only_property("ResultString", |lua, http: &mut HttpObject| {
                lua.create_string(http.session.result_text())
            })
            // baseunits/lua/LuaHTTPSend.pas:157-159
            .object("Headers", headers)
            .object("Cookies", cookies)
            .object("Document", document)
            // `MimeType`: the Content-Type of the body to send, and of the last response
            // (baseunits/lua/LuaHTTPSend.pas:160, baseunits/synapse/httpsend.pas:473-475,
            // :669-670).
            .property(
                "MimeType",
                |lua, http: &mut HttpObject| lua.create_string(http.session.mime_type()),
                |lua, http: &mut HttpObject, value: Value| {
                    http.session.set_mime_type(text(lua, value)?);
                    Ok(())
                },
            )
            // `UserAgent`, sent when non-empty (baseunits/lua/LuaHTTPSend.pas:161,
            // baseunits/synapse/httpsend.pas:478-479).
            .property(
                "UserAgent",
                |lua, http: &mut HttpObject| lua.create_string(http.session.user_agent()),
                |lua, http: &mut HttpObject, value: Value| {
                    http.session.set_user_agent(text(lua, value)?);
                    Ok(())
                },
            )
            // `RetryCount`: extra attempts after a failure, -1 for no limit
            // (baseunits/lua/LuaHTTPSend.pas:162, baseunits/httpsendthread.pas:624-626).
            // Assignment converts like `luaClassAddIntegerProperty`'s `lua_tointeger`
            // (baseunits/lua/LuaClass.pas:476-486).
            .property(
                "RetryCount",
                |_, http: &mut HttpObject| Ok(http.session.retry_count()),
                |lua, http: &mut HttpObject, value: Value| {
                    // Keeping only the low 32 bits of the Pascal `Integer` is the behaviour
                    // being reproduced.
                    let value = lua.coerce_integer(value)?.unwrap_or(0) as i32;
                    http.session.set_retry_count(value);
                    Ok(())
                },
            )
            // `EnabledCookies` (baseunits/lua/LuaHTTPSend.pas:163): when off, the module's jar
            // is neither read nor written and response cookies are dropped
            // (baseunits/httpsendthread.pas:475, :566-569). Assignment converts like
            // `lua_toboolean` (baseunits/lua/LuaClass.pas:488-498).
            .property(
                "EnabledCookies",
                |_, http: &mut HttpObject| Ok(http.session.enabled_cookies()),
                |_, http: &mut HttpObject, value: Value| {
                    http.session
                        .set_enabled_cookies(!matches!(value, Value::Nil | Value::Boolean(false)));
                    Ok(())
                },
            )
            .build(lua)
    }
}

/// `luaToString` (baseunits/lua/LuaUtils.pas:206) as text: strings and numbers convert,
/// anything else is empty.
fn text(lua: &Lua, value: Value) -> mlua::Result<String> {
    Ok(String::from_utf8_lossy(&to_bytes(lua, value)?).into_owned())
}

/// A module's HTTP overrides (`TWebsiteModuleSettings.HTTP`,
/// baseunits/WebsiteModulesSettings.pas:37-43), applied by [`create_http`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModuleHttpOverrides {
    /// Replaces the session's user agent when non-empty.
    pub user_agent: String,
    /// `name=value` pairs separated by `;`, merged into the cookies of every request.
    pub cookies: String,
    pub proxy: ProxyOverride,
}

/// The proxy part of [`ModuleHttpOverrides`] (`TProxyType`,
/// baseunits/WebsiteModulesSettings.pas:11).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ProxyOverride {
    /// Keep the default proxy.
    #[default]
    Default,
    /// No proxy for this module.
    Direct,
    /// This proxy for this module.
    Proxy(Proxy),
}

/// Where a module's HTTP overrides come from: the module's stored settings.
pub trait ModuleHttpSettings: Send + Sync {
    /// The module's HTTP overrides, or `None` while its settings are disabled
    /// (`Settings.Enabled`, baseunits/WebsiteModules.pas:280, :363). Read when a session is
    /// created and again for the cookies of every request.
    fn http_overrides(&self) -> Option<ModuleHttpOverrides>;
}

/// What a session needs from the module it runs for: the module's shared HTTP state (cookie
/// jar and connection queue) and its settings.
#[derive(Clone)]
pub struct HttpModule {
    pub http: ModuleHttp,
    pub settings: Arc<dyn ModuleHttpSettings>,
}

/// A new session for `module`, or a plain one without a module, set up like FMD2's
/// `TModuleContainer.CreateHTTP` and `PrepareHTTP` (baseunits/WebsiteModules.pas:353-387).
///
/// The session uses the module's cookie jar and connection queue, and before every request
/// merges the cookies of the module's settings (`MergeHTTPCookiesFromSetting`,
/// baseunits/WebsiteModules.pas:278-283). When the settings are enabled at creation, their
/// user agent replaces the default when non-empty, and their proxy type `Direct` turns the
/// proxy off while a proxy server replaces the default one (:363-379).
pub fn create_http(client: &HttpClient, module: Option<&HttpModule>) -> HttpSession {
    let Some(module) = module else {
        return client.session();
    };
    let mut session = client.session_for(&module.http);
    let settings = module.settings.clone();
    session.set_on_after_set_cookies(Some(Arc::new(move |session: &mut HttpSession| {
        if let Some(overrides) = settings.http_overrides()
            && !overrides.cookies.is_empty()
        {
            session.merge_cookies(&overrides.cookies);
        }
    })));
    if let Some(overrides) = module.settings.http_overrides() {
        if !overrides.user_agent.is_empty() {
            session.set_user_agent(overrides.user_agent);
        }
        match overrides.proxy {
            ProxyOverride::Default => {}
            // `SetNoProxy` (baseunits/httpsendthread.pas:909-912).
            ProxyOverride::Direct => session.set_proxy("", "", "", "", ""),
            ProxyOverride::Proxy(proxy) => {
                let kind = match proxy.kind {
                    ProxyKind::Http => "HTTP",
                    ProxyKind::Socks4 => "SOCKS4",
                    ProxyKind::Socks5 => "SOCKS5",
                };
                session.set_proxy(kind, &proxy.host, &proxy.port, &proxy.user, &proxy.pass);
            }
        }
    }
    session
}
