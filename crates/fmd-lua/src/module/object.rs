//! The `MODULE` object and its `Storage`, `Guardian` and `Account` sub-objects
//! (`luaWebsiteModuleAddMetaTable`, baseunits/lua/LuaWebsiteModules.pas:848-1040).

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::SystemTime;

use mlua::{AnyUserData, Lua, Value, Variadic};

use super::{Account, CriticalSection, Module, ModuleDef, ModuleOption, OptionKind, OptionValue};
use crate::LuaClass;
use crate::class::to_bytes;

type Class = LuaClass<Arc<Module>>;

/// Builds the `MODULE` object over `module`. `Account` is a member only when the module
/// supports accounts at this point (baseunits/lua/LuaWebsiteModules.pas:1035-1036), so the
/// object `Init` gets from `NewWebsiteModule()` never has one.
pub(crate) fn build_module(lua: &Lua, module: &Arc<Module>) -> mlua::Result<AnyUserData> {
    let state = Rc::new(RefCell::new(module.clone()));
    // baseunits/lua/LuaWebsiteModules.pas:996
    let mut class = Class::new(state).object("Guardian", build_guardian(lua, module.guardian())?);
    // baseunits/lua/LuaWebsiteModules.pas:997-1000, :1024 (`LastUpdated`)
    for (name, field) in STRING_PROPERTIES {
        class = string_property(class, name, *field);
    }
    // baseunits/lua/LuaWebsiteModules.pas:1001-1002, :1025 (`CurrentDirectoryIndex`), :1038
    // (`Tag`), :1028 (`TotalDirectory`, through `lua_tointeger` like the others, :876-886).
    for (name, field) in INTEGER_PROPERTIES {
        class = integer_property(class, name, *field);
    }
    // baseunits/lua/LuaWebsiteModules.pas:1006-1009
    for (name, field) in BOOLEAN_PROPERTIES {
        class = boolean_property(class, name, *field);
    }
    // baseunits/lua/LuaWebsiteModules.pas:1010-1023, :1026
    for (name, field) in CALLBACK_PROPERTIES {
        class = callback_property(class, name, *field);
    }
    class = class
        // `ConnectionsQueue.MaxConnections` (baseunits/lua/LuaWebsiteModules.pas:1003).
        .property(
            "MaxConnectionLimit",
            |_, m| Ok(m.def_read().max_connection_limit),
            |lua, m, value: Value| {
                let max = to_integer(lua, value)?;
                m.def_write().max_connection_limit = max;
                m.http.set_max_connections(u32::try_from(max).unwrap_or(0));
                Ok(())
            },
        )
        // baseunits/lua/LuaWebsiteModules.pas:1004; `IncActiveTaskCount` and
        // `DecActiveTaskCount` (baseunits/WebsiteModules.pas:388-396) are atomic.
        .property(
            "ActiveTaskCount",
            |_, m| Ok(m.active_task_count.load(Ordering::SeqCst)),
            |lua, m, value: Value| {
                let count = to_integer(lua, value)?;
                m.active_task_count.store(count, Ordering::SeqCst);
                Ok(())
            },
        )
        // `ConnectionsQueue.ActiveConnections` (baseunits/lua/LuaWebsiteModules.pas:1005). FMD2
        // lets modules overwrite the queue's counter; here assigning is ignored, so a module
        // cannot break the connection limit.
        .read_only_property("ActiveConnectionCount", |_, m| {
            Ok(i64::from(m.http.active_connections()))
        })
        // `lua_getaccountsupport`/`lua_setaccountsupport` (baseunits/lua/LuaWebsiteModules.pas:
        // 951-962, :1029) over `SetAccountSupport` (baseunits/WebsiteModules.pas:258-270): turning it on
        // creates the account, turning it off drops it.
        .property(
            "AccountSupport",
            |_, m| Ok(m.def_read().account_support),
            |_, m, value: Value| {
                let on = truthy(&value);
                m.def_write().account_support = on;
                let mut account = super::lock(&m.account);
                match (on, account.is_some()) {
                    (true, false) => *account = Some(Arc::default()),
                    (false, true) => *account = None,
                    _ => {}
                }
                Ok(())
            },
        );
    class = add_methods(class);
    // baseunits/lua/LuaWebsiteModules.pas:1033, :1035-1036
    class = class.object("Storage", build_storage(lua, module)?);
    if let Some(account) = module.account() {
        class = class.object("Account", build_account(lua, module, &account)?);
    }
    class.build(lua).map_err(mlua::Error::from)
}

/// The methods (baseunits/lua/LuaWebsiteModules.pas:964-975, :1031).
fn add_methods(class: Class) -> Class {
    class
        // `lua_addoptioncheckbox` (baseunits/lua/LuaWebsiteModules.pas:848-853).
        .method(
            "AddOptionCheckBox",
            |lua, m, (name, caption, default): (Value, Value, Value)| {
                let kind = OptionKind::CheckBox {
                    default: truthy(&default),
                };
                add_option(lua, m, name, caption, kind)
            },
        )
        // `lua_addoptionedit` (baseunits/lua/LuaWebsiteModules.pas:855-860).
        .method(
            "AddOptionEdit",
            |lua, m, (name, caption, default): (Value, Value, Value)| {
                let kind = OptionKind::Edit {
                    default: to_string(lua, default)?,
                };
                add_option(lua, m, name, caption, kind)
            },
        )
        // `lua_addoptionspinedit` (baseunits/lua/LuaWebsiteModules.pas:862-867).
        .method(
            "AddOptionSpinEdit",
            |lua, m, (name, caption, default): (Value, Value, Value)| {
                let kind = OptionKind::SpinEdit {
                    default: to_integer(lua, default)?,
                };
                add_option(lua, m, name, caption, kind)
            },
        )
        // `lua_addoptioncombobox` (baseunits/lua/LuaWebsiteModules.pas:869-874).
        .method(
            "AddOptionComboBox",
            |lua, m, (name, caption, items, default): (Value, Value, Value, Value)| {
                let kind = OptionKind::ComboBox {
                    items: to_string(lua, items)?,
                    default: to_integer(lua, default)?,
                };
                add_option(lua, m, name, caption, kind)
            },
        )
        // `lua_addservercookies` (baseunits/lua/LuaWebsiteModules.pas:888-895): `(url, cookies)`,
        // or `(cookies)` for no URL.
        .method("AddServerCookies", |lua, m, args: Variadic<Value>| {
            let (url, cookies) = one_or_two(lua, args, true)?;
            m.http
                .cookies()
                .add_server_cookies(&url, &cookies, SystemTime::now());
            m.save_cookies().map_err(mlua::Error::external)
        })
        // `lua_getservercookies` (baseunits/lua/LuaWebsiteModules.pas:897-904): `(domain, name)`
        // or `(domain)` for all of the domain's cookies.
        .method("GetServerCookies", |lua, m, args: Variadic<Value>| {
            let (domain, name) = one_or_two(lua, args, false)?;
            lua.create_string(m.http.cookies().get_server_cookies(&domain, &name))
        })
        // `lua_removecookies` (baseunits/lua/LuaWebsiteModules.pas:906-913).
        .method("RemoveCookies", |lua, m, args: Variadic<Value>| {
            let (domain, name) = one_or_two(lua, args, false)?;
            m.http.cookies().remove_cookies(&domain, &name);
            m.save_cookies().map_err(mlua::Error::external)
        })
        // `lua_clearcookies` (baseunits/lua/LuaWebsiteModules.pas:915-919).
        .method("ClearCookies", |_, m, ()| {
            m.http.cookies().clear();
            m.save_cookies().map_err(mlua::Error::external)
        })
        // `lua_getoption` (baseunits/lua/LuaWebsiteModules.pas:921-949): typed by the option's
        // kind, nil for an unknown option.
        .method("GetOption", |lua, m, name: Value| {
            let name = to_string(lua, name)?;
            Ok(
                match m.option_value(&name).map_err(mlua::Error::external)? {
                    None => Value::Nil,
                    Some(OptionValue::Bool(b)) => Value::Boolean(b),
                    Some(OptionValue::Text(s)) => Value::String(lua.create_string(s)?),
                    Some(OptionValue::Integer(i)) => Value::Integer(i64::from(i)),
                },
            )
        })
}

/// `TLuaWebsiteModule.AddOption` (baseunits/lua/LuaWebsiteModules.pas:757-766).
///
/// FMD2 keeps the options in a sorted, case-insensitive `TStringList` that ignores duplicates:
/// adding a name again finds the existing entry and replaces its caption, kind and default.
/// An option without a name is kept, so `GetOption('')` finds it, though it never reaches the
/// settings (baseunits/WebsiteModules.pas:427).
fn add_option(
    lua: &Lua,
    module: &mut Arc<Module>,
    name: Value,
    caption: Value,
    kind: OptionKind,
) -> mlua::Result<()> {
    let option = ModuleOption {
        name: to_string(lua, name)?,
        caption: to_string(lua, caption)?,
        kind,
    };
    let mut def = module.def_write();
    match def
        .options
        .iter_mut()
        .find(|o| o.name.eq_ignore_ascii_case(&option.name))
    {
        Some(existing) => {
            existing.caption = option.caption;
            existing.kind = option.kind;
        }
        None => def.options.push(option),
    }
    Ok(())
}

/// The two string arguments of the cookie methods, which branch on `lua_gettop(L) = 2`: with
/// two arguments they are taken as given; otherwise the first argument is the second string
/// when `first_is_second`, else the first (the other one is empty).
fn one_or_two(
    lua: &Lua,
    args: Variadic<Value>,
    first_is_second: bool,
) -> mlua::Result<(String, String)> {
    let mut args = args.into_iter();
    let first = to_string(lua, args.next().unwrap_or(Value::Nil))?;
    if let (Some(second), None) = (args.next(), args.next()) {
        return Ok((first, to_string(lua, second)?));
    }
    Ok(if first_is_second {
        (String::new(), first)
    } else {
        (first, String::new())
    })
}

/// `Storage` (`luaStringsStorageAddMetaTable`, baseunits/lua/LuaStringsStorage.pas:150-159),
/// one per module and shared by every state running it. FMD2's `TStringsStorage` locks writes
/// only; here reads lock too.
fn build_storage(lua: &Lua, module: &Arc<Module>) -> mlua::Result<AnyUserData> {
    LuaClass::new(Rc::new(RefCell::new(module.clone())))
        // `strings_remove` over `TStringsStorage.Remove` (:88-101, :124-128).
        .method("Remove", |lua, m: &mut Arc<Module>, name: Value| {
            let name = to_bytes(lua, name)?;
            let mut storage = super::lock(&m.storage);
            let i = storage.values.index_of_name(&name);
            // FMD2 exits when the name is missing (:93); `delete` refuses index -1 the same way.
            let _ = storage.values.delete(i);
            Ok(())
        })
        // `Free` and `Destroy` (:106-110, :145-146) free the storage in FMD2, leaving every state that
        // shares the module with a dangling object; here they do nothing.
        .method("Free", |_, _, ()| Ok(()))
        .method("Destroy", |_, _, ()| Ok(()))
        // `Values[name]` (:42-45, :62-70, :112-122), FPC's `TStrings.Values`.
        .default_array_property(
            |lua, m: &mut Arc<Module>, name: Value| {
                let name = to_bytes(lua, name)?;
                lua.create_string(super::lock(&m.storage).values.value(&name))
            },
            |lua, m: &mut Arc<Module>, name: Value, value: Value| {
                let (name, value) = (to_bytes(lua, name)?, to_bytes(lua, value)?);
                super::lock(&m.storage).values.set_value(&name, &value);
                Ok(())
            },
        )
        // `Text` (:47-60, :130-140).
        .property(
            "Text",
            |lua, m: &mut Arc<Module>| lua.create_string(super::lock(&m.storage).values.text()),
            |lua, m: &mut Arc<Module>, text: Value| {
                let text = to_bytes(lua, text)?;
                super::lock(&m.storage).values.set_text(&text);
                Ok(())
            },
        )
        // `Tag`, `Enable` and `Status` (:156-158).
        .property(
            "Tag",
            |_, m: &mut Arc<Module>| Ok(super::lock(&m.storage).tag),
            |lua, m: &mut Arc<Module>, value: Value| {
                let tag = to_integer(lua, value)?;
                super::lock(&m.storage).tag = tag;
                Ok(())
            },
        )
        .property(
            "Enable",
            |_, m: &mut Arc<Module>| Ok(super::lock(&m.storage).enable),
            |_, m: &mut Arc<Module>, value: Value| {
                super::lock(&m.storage).enable = truthy(&value);
                Ok(())
            },
        )
        .property(
            "Status",
            |lua, m: &mut Arc<Module>| lua.create_string(&super::lock(&m.storage).status),
            |lua, m: &mut Arc<Module>, value: Value| {
                let status = to_bytes(lua, value)?;
                super::lock(&m.storage).status = status;
                Ok(())
            },
        )
        .build(lua)
        .map_err(mlua::Error::from)
}

/// A `Guardian` (`luaCriticalSectionAddMetaTable`, baseunits/lua/LuaCriticalSection.pas:21-54).
fn build_guardian(lua: &Lua, guardian: &Arc<CriticalSection>) -> mlua::Result<AnyUserData> {
    LuaClass::new(Rc::new(RefCell::new(guardian.clone())))
        // baseunits/lua/LuaCriticalSection.pas:21-25
        .method("TryEnter", |_, g: &mut Arc<CriticalSection>, ()| {
            Ok(g.try_enter())
        })
        // baseunits/lua/LuaCriticalSection.pas:27-31
        .method("Enter", |_, g: &mut Arc<CriticalSection>, ()| {
            g.enter();
            Ok(())
        })
        // baseunits/lua/LuaCriticalSection.pas:33-37
        .method("Leave", |_, g: &mut Arc<CriticalSection>, ()| {
            g.leave();
            Ok(())
        })
        .build(lua)
        .map_err(mlua::Error::from)
}

/// `Account` (`luaWebsiteModuleAccountAddMetaTable`, baseunits/lua/LuaWebsiteModules.pas:977-989).
/// FMD2 saves the account with the rest of `modules.json` when it saves the settings
/// (baseunits/WebsiteModules.pas:665-675); here every assignment is written to the module's
/// settings store right away, so what a callback sets survives a restart.
fn build_account(
    lua: &Lua,
    module: &Arc<Module>,
    account: &Arc<Account>,
) -> mlua::Result<AnyUserData> {
    let guardian = build_guardian(lua, account.guardian())?;
    LuaClass::new(Rc::new(RefCell::new((module.clone(), account.clone()))))
        .property(
            "Enabled",
            |_, (_, a): &mut AccountRef| Ok(super::lock(&a.state).enabled),
            |_, (m, a): &mut AccountRef, value: Value| {
                super::lock(&a.state).enabled = truthy(&value);
                m.save_account().map_err(mlua::Error::external)
            },
        )
        .property(
            "Username",
            |lua, (_, a): &mut AccountRef| lua.create_string(&super::lock(&a.state).username),
            |lua, (m, a): &mut AccountRef, value: Value| {
                let value = to_string(lua, value)?;
                super::lock(&a.state).username = value;
                m.save_account().map_err(mlua::Error::external)
            },
        )
        .property(
            "Password",
            |lua, (_, a): &mut AccountRef| lua.create_string(&super::lock(&a.state).password),
            |lua, (m, a): &mut AccountRef, value: Value| {
                let value = to_string(lua, value)?;
                super::lock(&a.state).password = value;
                m.save_account().map_err(mlua::Error::external)
            },
        )
        .property(
            "Status",
            |_, (_, a): &mut AccountRef| Ok(super::lock(&a.state).status),
            |lua, (m, a): &mut AccountRef, value: Value| {
                let value = to_integer(lua, value)?;
                super::lock(&a.state).status = value;
                m.save_account().map_err(mlua::Error::external)
            },
        )
        .property(
            "Cookies",
            |lua, (_, a): &mut AccountRef| lua.create_string(&super::lock(&a.state).cookies),
            |lua, (m, a): &mut AccountRef, value: Value| {
                let value = to_string(lua, value)?;
                super::lock(&a.state).cookies = value;
                m.save_account().map_err(mlua::Error::external)
            },
        )
        .object("Guardian", guardian)
        .build(lua)
        .map_err(mlua::Error::from)
}

/// The state behind an `Account` object: the module it belongs to and the account itself.
type AccountRef = (Arc<Module>, Arc<Account>);

type StringField = fn(&mut ModuleDef) -> &mut String;
type IntegerField = fn(&mut ModuleDef) -> &mut i32;
type BooleanField = fn(&mut ModuleDef) -> &mut bool;
type CallbackField = fn(&mut ModuleDef) -> &mut Option<String>;

const STRING_PROPERTIES: &[(&str, StringField)] = &[
    ("ID", |d| &mut d.id),
    ("Name", |d| &mut d.name),
    ("RootURL", |d| &mut d.root_url),
    ("Category", |d| &mut d.category),
    ("LastUpdated", |d| &mut d.last_updated),
];

const INTEGER_PROPERTIES: &[(&str, IntegerField)] = &[
    ("MaxTaskLimit", |d| &mut d.max_task_limit),
    ("MaxThreadPerTaskLimit", |d| {
        &mut d.max_thread_per_task_limit
    }),
    ("CurrentDirectoryIndex", |d| &mut d.current_directory_index),
    ("TotalDirectory", |d| &mut d.total_directory),
    ("Tag", |d| &mut d.tag),
];

const BOOLEAN_PROPERTIES: &[(&str, BooleanField)] = &[
    ("SortedList", |d| &mut d.sorted_list),
    ("InformationAvailable", |d| &mut d.information_available),
    ("FavoriteAvailable", |d| &mut d.favorite_available),
    ("DynamicPageLink", |d| &mut d.dynamic_page_link),
];

const CALLBACK_PROPERTIES: &[(&str, CallbackField)] = &[
    ("OnBeforeUpdateList", |d| &mut d.on_before_update_list),
    ("OnAfterUpdateList", |d| &mut d.on_after_update_list),
    ("OnGetDirectoryPageNumber", |d| {
        &mut d.on_get_directory_page_number
    }),
    ("OnGetNameAndLink", |d| &mut d.on_get_name_and_link),
    ("OnGetInfo", |d| &mut d.on_get_info),
    ("OnTaskStart", |d| &mut d.on_task_start),
    ("OnGetPageNumber", |d| &mut d.on_get_page_number),
    ("OnGetImageURL", |d| &mut d.on_get_image_url),
    ("OnBeforeDownloadImage", |d| &mut d.on_before_download_image),
    ("OnDownloadImage", |d| &mut d.on_download_image),
    ("OnSaveImage", |d| &mut d.on_save_image),
    ("OnAfterImageSaved", |d| &mut d.on_after_image_saved),
    ("OnLogin", |d| &mut d.on_login),
    ("OnAccountState", |d| &mut d.on_account_state),
    ("OnCheckSite", |d| &mut d.on_check_site),
];

/// A string property (`luaClassAddStringProperty`, baseunits/lua/LuaClass.pas:520-524,
/// accessors :464-474):
/// assigning converts like `luaToString`.
fn string_property(class: Class, name: &str, field: StringField) -> Class {
    class.property(
        name,
        move |lua, m| lua.create_string(field(&mut m.def_write()).as_bytes()),
        move |lua, m, value: Value| {
            let value = to_string(lua, value)?;
            *field(&mut m.def_write()) = value;
            Ok(())
        },
    )
}

/// An integer property (`luaClassAddIntegerProperty`, baseunits/lua/LuaClass.pas:526-530,
/// accessors :476-486):
/// assigning converts like `lua_tointeger`.
fn integer_property(class: Class, name: &str, field: IntegerField) -> Class {
    class.property(
        name,
        move |_, m| Ok(*field(&mut m.def_write())),
        move |lua, m, value: Value| {
            let value = to_integer(lua, value)?;
            *field(&mut m.def_write()) = value;
            Ok(())
        },
    )
}

/// A boolean property (`luaClassAddBooleanProperty`, baseunits/lua/LuaClass.pas:532-536,
/// accessors :488-498):
/// assigning converts like `lua_toboolean`.
fn boolean_property(class: Class, name: &str, field: BooleanField) -> Class {
    class.property(
        name,
        move |_, m| Ok(*field(&mut m.def_write())),
        move |_, m, value: Value| {
            *field(&mut m.def_write()) = truthy(&value);
            Ok(())
        },
    )
}

/// A callback name: a string property whose empty value means no callback.
fn callback_property(class: Class, name: &str, field: CallbackField) -> Class {
    class.property(
        name,
        move |lua, m| {
            lua.create_string(
                field(&mut m.def_write())
                    .as_deref()
                    .unwrap_or_default()
                    .as_bytes(),
            )
        },
        move |lua, m, value: Value| {
            let value = to_string(lua, value)?;
            *field(&mut m.def_write()) = (!value.is_empty()).then_some(value);
            Ok(())
        },
    )
}

/// `luaToString` (baseunits/lua/LuaUtils.pas:206) into a Rust string; invalid UTF-8 is
/// replaced.
fn to_string(lua: &Lua, value: Value) -> mlua::Result<String> {
    Ok(String::from_utf8_lossy(&to_bytes(lua, value)?).into_owned())
}

/// `lua_tointeger`: integral numbers and numeric strings convert, anything else is 0. Like the
/// Pascal `Integer` it lands in, only the low 32 bits are kept.
fn to_integer(lua: &Lua, value: Value) -> mlua::Result<i32> {
    // Keeping only the low 32 bits is the behaviour being reproduced.
    Ok(lua.coerce_integer(value)?.unwrap_or(0) as i32)
}

/// `lua_toboolean`: only `nil` and `false` are false.
fn truthy(value: &Value) -> bool {
    !matches!(value, Value::Nil | Value::Boolean(false))
}
