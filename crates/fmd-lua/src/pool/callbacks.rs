//! The callbacks a module declares, the globals each one gets and what is read back afterwards,
//! reproducing the `Do*` functions of baseunits/lua/LuaWebsiteModules.pas:154-465.

use std::cell::RefCell;
use std::rc::Rc;

use mlua::{AnyUserData, Lua, Value};

use super::JobError;
use super::worker::Ctx;
use crate::module::ModuleDef;
use crate::{LuaClass, LuaStrings};

/// The callbacks of a module (`TLuaWebsiteModule.On*`, baseunits/lua/LuaWebsiteModules.pas:
/// 15-54).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Callback {
    OnBeforeUpdateList,
    OnAfterUpdateList,
    OnGetDirectoryPageNumber,
    OnGetNameAndLink,
    OnGetInfo,
    OnTaskStart,
    OnGetPageNumber,
    OnGetImageURL,
    OnBeforeDownloadImage,
    OnDownloadImage,
    OnSaveImage,
    OnAfterImageSaved,
    OnLogin,
    OnAccountState,
    OnCheckSite,
}

impl Callback {
    /// The name of the Lua function the module declared for this callback.
    pub fn function(self, def: &ModuleDef) -> Option<&str> {
        let name = match self {
            Callback::OnBeforeUpdateList => &def.on_before_update_list,
            Callback::OnAfterUpdateList => &def.on_after_update_list,
            Callback::OnGetDirectoryPageNumber => &def.on_get_directory_page_number,
            Callback::OnGetNameAndLink => &def.on_get_name_and_link,
            Callback::OnGetInfo => &def.on_get_info,
            Callback::OnTaskStart => &def.on_task_start,
            Callback::OnGetPageNumber => &def.on_get_page_number,
            Callback::OnGetImageURL => &def.on_get_image_url,
            Callback::OnBeforeDownloadImage => &def.on_before_download_image,
            Callback::OnDownloadImage => &def.on_download_image,
            Callback::OnSaveImage => &def.on_save_image,
            Callback::OnAfterImageSaved => &def.on_after_image_saved,
            Callback::OnLogin => &def.on_login,
            Callback::OnAccountState => &def.on_account_state,
            Callback::OnCheckSite => &def.on_check_site,
        };
        name.as_deref()
    }
}

impl std::fmt::Display for Callback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}

/// The fields of FMD2's `TMangaInfo` a module sees through `MANGAINFO`
/// (baseunits/uBaseUnit.pas:322-337, baseunits/lua/LuaMangaInfo.pas:18-37).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MangaInfo {
    pub url: String,
    pub title: String,
    pub alt_titles: String,
    pub link: String,
    pub cover_link: String,
    pub authors: String,
    pub artists: String,
    pub genres: String,
    pub status: String,
    pub summary: String,
    pub chapter_names: Vec<String>,
    pub chapter_links: Vec<String>,
}

/// What `OnGetInfo` produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InfoReply {
    /// `no_error`, `net_problem` or `information_not_found`, as the callback returned it.
    pub status: u8,
    pub info: MangaInfo,
}

/// What a module sees of FMD2's update-list manager through `UPDATELIST`
/// (baseunits/lua/LuaUpdateListManager.pas:18-42).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UpdateList {
    /// `CurrentDirectoryPageNumber`.
    pub current_directory_page_number: i32,
    /// The text last passed to `UpdateStatusText`, which FMD2 shows in its status bar
    /// (`UpdateStatusFormatted`, baseunits/uUpdateThread.pas:614-624).
    pub status_text: Option<String>,
}

/// What `OnBeforeUpdateList` or `OnAfterUpdateList` produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListReply {
    pub ok: bool,
    pub list: UpdateList,
}

/// What `OnGetDirectoryPageNumber` produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageCount {
    /// `no_error`, `net_problem` or `information_not_found`, as the callback returned it.
    pub status: u8,
    /// The number of directory pages: the global `PAGENUMBER` as the callback left it, or the
    /// page passed in when the callback set it to `nil`.
    pub page: i32,
    pub list: UpdateList,
}

/// What `OnGetNameAndLink` produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamesAndLinks {
    /// `no_error`, `net_problem` or `information_not_found`, as the callback returned it.
    pub status: u8,
    pub names: Vec<String>,
    pub links: Vec<String>,
    pub list: UpdateList,
}

/// What a module sees of a download task through `TASK` (`TTaskContainer`,
/// baseunits/lua/LuaDownloadTask.pas:18-34).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Task {
    pub page_links: Vec<String>,
    pub page_container_links: Vec<String>,
    pub file_names: Vec<String>,
    pub chapter_links: Vec<String>,
    pub chapter_names: Vec<String>,
    pub current_download_chapter_ptr: i32,
    pub page_number: i32,
    /// The task thread's `CurrentMaxFileNameLength`.
    pub current_max_file_name_length: i32,
    /// `DownloadInfo.Link`: the manga's link.
    pub link: String,
}

/// What a download callback produced: its boolean result and the task as it left it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskReply {
    pub ok: bool,
    pub task: Task,
}

/// A callback to run, with its arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    BeforeUpdateList {
        list: UpdateList,
    },
    /// `PAGENUMBER` starts as `page`, `WORKPTR` as `work_ptr`.
    GetDirectoryPageNumber {
        list: UpdateList,
        page: i32,
        work_ptr: i32,
    },
    /// `URL` is `page_index` as a string, as the update-list thread passes it.
    GetNameAndLink {
        list: UpdateList,
        page_index: i32,
    },
    AfterUpdateList {
        list: UpdateList,
    },
    GetInfo {
        url: String,
    },
    TaskStart {
        task: Task,
    },
    /// `URL` is the chapter's URL.
    GetPageNumber {
        task: Task,
        url: String,
    },
    /// `WORKID` is the page's index, `URL` its URL.
    GetImageUrl {
        task: Task,
        work_id: i32,
        url: String,
    },
    /// `WORKID` is the page's index, `URL` the image's URL.
    BeforeDownloadImage {
        task: Task,
        work_id: i32,
        url: String,
    },
    /// `WORKID` is the page's index, `URL` the image's URL.
    DownloadImage {
        task: Task,
        work_id: i32,
        url: String,
    },
    /// `PATH` is the chapter's directory, `FILENAME` the image's file name without extension.
    SaveImage {
        path: String,
        name: String,
    },
    /// `FILENAME` is the saved file.
    AfterImageSaved {
        file_name: String,
    },
    Login,
    AccountState,
    CheckSite,
}

impl Call {
    /// The callback this call runs.
    pub fn callback(&self) -> Callback {
        match self {
            Call::BeforeUpdateList { .. } => Callback::OnBeforeUpdateList,
            Call::GetDirectoryPageNumber { .. } => Callback::OnGetDirectoryPageNumber,
            Call::GetNameAndLink { .. } => Callback::OnGetNameAndLink,
            Call::AfterUpdateList { .. } => Callback::OnAfterUpdateList,
            Call::GetInfo { .. } => Callback::OnGetInfo,
            Call::TaskStart { .. } => Callback::OnTaskStart,
            Call::GetPageNumber { .. } => Callback::OnGetPageNumber,
            Call::GetImageUrl { .. } => Callback::OnGetImageURL,
            Call::BeforeDownloadImage { .. } => Callback::OnBeforeDownloadImage,
            Call::DownloadImage { .. } => Callback::OnDownloadImage,
            Call::SaveImage { .. } => Callback::OnSaveImage,
            Call::AfterImageSaved { .. } => Callback::OnAfterImageSaved,
            Call::Login => Callback::OnLogin,
            Call::AccountState => Callback::OnAccountState,
            Call::CheckSite => Callback::OnCheckSite,
        }
    }
}

/// What a [`Call`] produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    BeforeUpdateList(ListReply),
    GetDirectoryPageNumber(PageCount),
    GetNameAndLink(NamesAndLinks),
    AfterUpdateList(ListReply),
    GetInfo(InfoReply),
    TaskStart(TaskReply),
    GetPageNumber(TaskReply),
    GetImageUrl(TaskReply),
    BeforeDownloadImage(TaskReply),
    DownloadImage(TaskReply),
    /// The file the image was saved to; empty when it was not.
    SaveImage(String),
    AfterImageSaved(bool),
    Login(bool),
    AccountState(bool),
    CheckSite(bool),
}

/// Runs `call` in `ctx`'s state: sets its globals, calls the module's function, reads back.
pub(super) fn run(ctx: &mut Ctx<'_>, call: Call) -> Result<Answer, JobError> {
    let callback = call.callback();
    match call {
        // `DoBeforeUpdateList` and `DoAfterUpdateList`
        // (baseunits/lua/LuaWebsiteModules.pas:154-188).
        Call::BeforeUpdateList { list } | Call::AfterUpdateList { list } => {
            let list = ListObject::new(list);
            let lua = ctx.lua().clone();
            ctx.setup(callback, |_| {
                lua.globals().set("UPDATELIST", list.build(&lua)?)
            })?;
            let top = ctx.call(callback)?;
            let reply = ListReply {
                ok: to_boolean(&top),
                list: list.read(),
            };
            Ok(match callback {
                Callback::OnBeforeUpdateList => Answer::BeforeUpdateList(reply),
                _ => Answer::AfterUpdateList(reply),
            })
        }
        // `DoGetDirectoryPageNumber` (baseunits/lua/LuaWebsiteModules.pas:190-217).
        Call::GetDirectoryPageNumber {
            list,
            page,
            work_ptr,
        } => {
            let list = ListObject::new(list);
            let lua = ctx.lua().clone();
            ctx.setup(callback, |ctx| {
                push_net_status(&lua)?;
                ctx.set_http()?;
                let g = lua.globals();
                g.set("UPDATELIST", list.build(&lua)?)?;
                g.set("PAGENUMBER", page)?;
                g.set("WORKPTR", work_ptr)
            })?;
            let top = ctx.call(callback)?;
            let status = to_byte(&lua, top);
            // `lua_getglobal` leaves `PAGENUMBER` on top of the stack.
            let global = ctx.push_global(callback, |lua| lua.globals().get("PAGENUMBER"))?;
            let page = match global {
                Value::Nil => page,
                // Keeping only the low 32 bits of the Pascal `Integer` is the behaviour being
                // reproduced.
                value => to_integer(&lua, value) as i32,
            };
            Ok(Answer::GetDirectoryPageNumber(PageCount {
                status,
                page,
                list: list.read(),
            }))
        }
        // `DoGetNameAndLink` (baseunits/lua/LuaWebsiteModules.pas:219-243).
        Call::GetNameAndLink { list, page_index } => {
            let (names, links) = (LuaStrings::new(), LuaStrings::new());
            let list = ListObject::new(list);
            let lua = ctx.lua().clone();
            ctx.setup(callback, |ctx| {
                push_net_status(&lua)?;
                ctx.set_http()?;
                let g = lua.globals();
                g.set("NAMES", names.build(&lua)?)?;
                g.set("LINKS", links.build(&lua)?)?;
                g.set("UPDATELIST", list.build(&lua)?)?;
                g.set("URL", page_index.to_string())
            })?;
            let top = ctx.call(callback)?;
            Ok(Answer::GetNameAndLink(NamesAndLinks {
                status: to_byte(&lua, top),
                names: read_strings(&names),
                links: read_strings(&links),
                list: list.read(),
            }))
        }
        // `DoGetInfo` (baseunits/lua/LuaWebsiteModules.pas:245-265). `MANGAINFO.URL` is the
        // URL with the module's host, as `GetInfoFromURL` fills it first (baseunits/uData.pas:105).
        Call::GetInfo { url } => {
            let root_url = ctx.module().def().root_url;
            let info = MangaInfo {
                url: fill_host(&root_url, &url),
                ..MangaInfo::default()
            };
            let object = InfoObject::new(&info);
            let lua = ctx.lua().clone();
            ctx.setup(callback, |ctx| {
                push_net_status(&lua)?;
                lua.globals().set("MANGAINFO", object.build(&lua)?)?;
                ctx.set_http()?;
                lua.globals().set("URL", url.as_str())
            })?;
            let top = ctx.call(callback)?;
            Ok(Answer::GetInfo(InfoReply {
                status: to_byte(&lua, top),
                info: object.read(),
            }))
        }
        // `DoGetPageNumber` (baseunits/lua/LuaWebsiteModules.pas:285-304).
        Call::GetPageNumber { task, url } => {
            let task = TaskObject::new(&task);
            let lua = ctx.lua().clone();
            ctx.setup(callback, |ctx| {
                lua.globals().set("TASK", task.build(&lua)?)?;
                ctx.set_http()?;
                lua.globals().set("URL", url.as_str())
            })?;
            let top = ctx.call(callback)?;
            Ok(Answer::GetPageNumber(TaskReply {
                ok: to_boolean(&top),
                task: task.read(),
            }))
        }
        // `DoTaskStart` (baseunits/lua/LuaWebsiteModules.pas:267-283).
        Call::TaskStart { task } => {
            let task = TaskObject::new(&task);
            let lua = ctx.lua().clone();
            ctx.setup(callback, |_| lua.globals().set("TASK", task.build(&lua)?))?;
            let top = ctx.call(callback)?;
            Ok(Answer::TaskStart(TaskReply {
                ok: to_boolean(&top),
                task: task.read(),
            }))
        }
        // `DoGetImageURL`, `DoBeforeDownloadImage` and `DoDownloadImage`
        // (baseunits/lua/LuaWebsiteModules.pas:306-370).
        Call::GetImageUrl { task, work_id, url }
        | Call::BeforeDownloadImage { task, work_id, url }
        | Call::DownloadImage { task, work_id, url } => {
            let task = TaskObject::new(&task);
            let lua = ctx.lua().clone();
            ctx.setup(callback, |ctx| {
                lua.globals().set("TASK", task.build(&lua)?)?;
                ctx.set_http()?;
                lua.globals().set("WORKID", work_id)?;
                lua.globals().set("URL", url.as_str())
            })?;
            let top = ctx.call(callback)?;
            let reply = TaskReply {
                ok: to_boolean(&top),
                task: task.read(),
            };
            Ok(match callback {
                Callback::OnGetImageURL => Answer::GetImageUrl(reply),
                Callback::OnBeforeDownloadImage => Answer::BeforeDownloadImage(reply),
                _ => Answer::DownloadImage(reply),
            })
        }
        // `DoSaveImage` (baseunits/lua/LuaWebsiteModules.pas:372-391): the result through
        // `luaToString`, so anything but a string or number is `''`.
        Call::SaveImage { path, name } => {
            let lua = ctx.lua().clone();
            ctx.setup(callback, |ctx| {
                ctx.set_http()?;
                lua.globals().set("PATH", path.as_str())?;
                lua.globals().set("FILENAME", name.as_str())
            })?;
            let top = ctx.call(callback)?;
            let saved = crate::class::to_bytes(&lua, top)
                .map_err(|e| ctx.error(callback, e.to_string(), String::new()))?;
            Ok(Answer::SaveImage(text(&saved)))
        }
        // `DoAfterImageSaved` (baseunits/lua/LuaWebsiteModules.pas:393-410).
        Call::AfterImageSaved { file_name } => {
            let lua = ctx.lua().clone();
            ctx.setup(callback, |_| {
                lua.globals().set("FILENAME", file_name.as_str())
            })?;
            let top = ctx.call(callback)?;
            Ok(Answer::AfterImageSaved(to_boolean(&top)))
        }
        // `DoLogin` (baseunits/lua/LuaWebsiteModules.pas:412-429).
        Call::Login => {
            let lua = ctx.lua().clone();
            ctx.setup(callback, |ctx| {
                push_account_status(&lua)?;
                ctx.set_http()
            })?;
            let top = ctx.call(callback)?;
            Ok(Answer::Login(to_boolean(&top)))
        }
        // `DoAccountState` and `DoCheckSite` (baseunits/lua/LuaWebsiteModules.pas:431-465).
        Call::AccountState | Call::CheckSite => {
            let lua = ctx.lua().clone();
            ctx.setup(callback, |_| push_account_status(&lua))?;
            let ok = to_boolean(&ctx.call(callback)?);
            Ok(match callback {
                Callback::OnAccountState => Answer::AccountState(ok),
                _ => Answer::CheckSite(ok),
            })
        }
    }
}

/// Sets every global and object any callback gets, with empty values.
pub(super) fn install_every_global(lua: &Lua) -> mlua::Result<()> {
    push_net_status(lua)?;
    push_account_status(lua)?;
    let g = lua.globals();
    g.set(
        "MANGAINFO",
        InfoObject::new(&MangaInfo::default()).build(lua)?,
    )?;
    g.set("TASK", TaskObject::new(&Task::default()).build(lua)?)?;
    g.set(
        "UPDATELIST",
        ListObject::new(UpdateList::default()).build(lua)?,
    )?;
    g.set("NAMES", LuaStrings::new().build(lua)?)?;
    g.set("LINKS", LuaStrings::new().build(lua)?)?;
    for name in ["PAGENUMBER", "WORKPTR", "WORKID"] {
        g.set(name, 0)?;
    }
    for name in ["URL", "PATH", "FILENAME"] {
        g.set(name, "")?;
    }
    Ok(())
}

/// `LuaPushAccountStatus` (baseunits/lua/LuaWebsiteModules.pas:832-838): the ordinals of
/// `TAccountStatus` (baseunits/WebsiteModules.pas:78).
fn push_account_status(lua: &Lua) -> mlua::Result<()> {
    let g = lua.globals();
    g.set("asUnknown", 0)?;
    g.set("asChecking", 1)?;
    g.set("asValid", 2)?;
    g.set("asInvalid", 3)
}

/// `lua_toboolean`: only `nil` and `false` are false.
fn to_boolean(value: &Value) -> bool {
    !matches!(value, Value::Nil | Value::Boolean(false))
}

/// `LuaPushNetStatus` (baseunits/lua/LuaWebsiteModules.pas:825-830).
fn push_net_status(lua: &Lua) -> mlua::Result<()> {
    let g = lua.globals();
    g.set("no_error", 0)?;
    g.set("net_problem", 1)?;
    g.set("information_not_found", 2)
}

/// A `Byte` result read with `lua_tointeger` (e.g. baseunits/lua/LuaWebsiteModules.pas:260):
/// 0 for a value that is no integer; the Pascal assignment keeps the low 8 bits.
fn to_byte(lua: &Lua, value: Value) -> u8 {
    // Keeping only the low 8 bits is the behaviour being reproduced.
    to_integer(lua, value) as u8
}

/// `lua_tointeger`: integers, integral floats and strings holding one convert, anything else
/// is 0.
fn to_integer(lua: &Lua, value: Value) -> i64 {
    lua.coerce_integer(value).ok().flatten().unwrap_or(0)
}

/// `FillHost` (baseunits/uBaseUnit.pas:926-932): the path of `url` (`SplitURL`) after `host`
/// without its trailing slashes (`RemoveURLDelim`, baseunits/uBaseUnit.pas:2003-2006).
fn fill_host(host: &str, url: &str) -> String {
    let (_, path) = fmd_http::split_url_bytes(url.as_bytes());
    let host = host.trim_end_matches('/');
    format!("{host}{}", String::from_utf8_lossy(&path))
}

fn bytes(text: &str) -> Vec<u8> {
    text.as_bytes().to_vec()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// A Lua TStrings object over a copy of `items`.
fn strings(items: &[String]) -> LuaStrings {
    let list = LuaStrings::new();
    for item in items {
        list.list().borrow_mut().add(bytes(item));
    }
    list
}

fn read_strings(list: &LuaStrings) -> Vec<String> {
    list.list()
        .borrow()
        .items()
        .iter()
        .map(|i| text(i))
        .collect()
}

/// The state behind `MANGAINFO`: its string fields as bytes, as Lua sees them.
#[derive(Default)]
struct InfoFields {
    url: Vec<u8>,
    title: Vec<u8>,
    alt_titles: Vec<u8>,
    link: Vec<u8>,
    cover_link: Vec<u8>,
    authors: Vec<u8>,
    artists: Vec<u8>,
    genres: Vec<u8>,
    status: Vec<u8>,
    summary: Vec<u8>,
}

/// `MANGAINFO` (`luaMangaInfoAddMetaTable`, baseunits/lua/LuaMangaInfo.pas:18-37).
struct InfoObject {
    fields: Rc<RefCell<InfoFields>>,
    chapter_names: LuaStrings,
    chapter_links: LuaStrings,
}

impl InfoObject {
    fn new(info: &MangaInfo) -> InfoObject {
        InfoObject {
            fields: Rc::new(RefCell::new(InfoFields {
                url: bytes(&info.url),
                title: bytes(&info.title),
                alt_titles: bytes(&info.alt_titles),
                link: bytes(&info.link),
                cover_link: bytes(&info.cover_link),
                authors: bytes(&info.authors),
                artists: bytes(&info.artists),
                genres: bytes(&info.genres),
                status: bytes(&info.status),
                summary: bytes(&info.summary),
            })),
            chapter_names: strings(&info.chapter_names),
            chapter_links: strings(&info.chapter_links),
        }
    }

    fn build(&self, lua: &Lua) -> mlua::Result<AnyUserData> {
        let object = LuaClass::new(self.fields.clone())
            .string_property("URL", |f: &mut InfoFields| &mut f.url)
            .string_property("Title", |f: &mut InfoFields| &mut f.title)
            .string_property("AltTitles", |f: &mut InfoFields| &mut f.alt_titles)
            .string_property("Link", |f: &mut InfoFields| &mut f.link)
            .string_property("CoverLink", |f: &mut InfoFields| &mut f.cover_link)
            .string_property("Authors", |f: &mut InfoFields| &mut f.authors)
            .string_property("Artists", |f: &mut InfoFields| &mut f.artists)
            .string_property("Genres", |f: &mut InfoFields| &mut f.genres)
            .string_property("Status", |f: &mut InfoFields| &mut f.status)
            .string_property("Summary", |f: &mut InfoFields| &mut f.summary)
            .object("ChapterNames", self.chapter_names.build(lua)?)
            .object("ChapterLinks", self.chapter_links.build(lua)?)
            .build(lua)?;
        Ok(object)
    }

    fn read(&self) -> MangaInfo {
        let f = self.fields.borrow();
        MangaInfo {
            url: text(&f.url),
            title: text(&f.title),
            alt_titles: text(&f.alt_titles),
            link: text(&f.link),
            cover_link: text(&f.cover_link),
            authors: text(&f.authors),
            artists: text(&f.artists),
            genres: text(&f.genres),
            status: text(&f.status),
            summary: text(&f.summary),
            chapter_names: read_strings(&self.chapter_names),
            chapter_links: read_strings(&self.chapter_links),
        }
    }
}

/// `UPDATELIST` (`luaUpdateListManagerAddMetaTable`,
/// baseunits/lua/LuaUpdateListManager.pas:18-42).
struct ListObject(Rc<RefCell<UpdateList>>);

impl ListObject {
    fn new(list: UpdateList) -> ListObject {
        ListObject(Rc::new(RefCell::new(list)))
    }

    fn build(&self, lua: &Lua) -> mlua::Result<AnyUserData> {
        let object = LuaClass::new(self.0.clone())
            // `lua_GetCurrentDirectoryPageNumber`/`lua_SetCurrentDirectoryPageNumber`
            // (baseunits/lua/LuaUpdateListManager.pas:18-29), the latter through
            // `lua_tointeger`.
            .property(
                "CurrentDirectoryPageNumber",
                |_, list: &mut UpdateList| Ok(list.current_directory_page_number),
                |lua, list: &mut UpdateList, value: Value| {
                    // Keeping only the low 32 bits of the Pascal `Integer` is the behaviour
                    // being reproduced.
                    list.current_directory_page_number = to_integer(lua, value) as i32;
                    Ok(())
                },
            )
            // `lua_updateStatusText` (baseunits/lua/LuaUpdateListManager.pas:31-35), whose
            // argument goes through `luaToString`.
            .method(
                "UpdateStatusText",
                |lua, list: &mut UpdateList, value: Value| {
                    let value = crate::class::to_bytes(lua, value)?;
                    list.status_text = Some(text(&value));
                    Ok(())
                },
            )
            .build(lua)?;
        Ok(object)
    }

    fn read(&self) -> UpdateList {
        self.0.borrow().clone()
    }
}

/// The integer and string fields behind `TASK`.
struct TaskFields {
    current_download_chapter_ptr: i32,
    page_number: i32,
    current_max_file_name_length: i32,
    link: Vec<u8>,
}

/// `TASK` (`luaDownloadTaskMetaTable`, baseunits/lua/LuaDownloadTask.pas:18-34).
struct TaskObject {
    fields: Rc<RefCell<TaskFields>>,
    page_links: LuaStrings,
    chapter_links: LuaStrings,
    chapter_names: LuaStrings,
    page_container_links: LuaStrings,
    file_names: LuaStrings,
}

impl TaskObject {
    fn new(task: &Task) -> TaskObject {
        TaskObject {
            fields: Rc::new(RefCell::new(TaskFields {
                current_download_chapter_ptr: task.current_download_chapter_ptr,
                page_number: task.page_number,
                current_max_file_name_length: task.current_max_file_name_length,
                link: bytes(&task.link),
            })),
            page_links: strings(&task.page_links),
            chapter_links: strings(&task.chapter_links),
            chapter_names: strings(&task.chapter_names),
            page_container_links: strings(&task.page_container_links),
            file_names: strings(&task.file_names),
        }
    }

    fn build(&self, lua: &Lua) -> mlua::Result<AnyUserData> {
        let object = LuaClass::new(self.fields.clone())
            .object("PageLinks", self.page_links.build(lua)?)
            .object("ChapterLinks", self.chapter_links.build(lua)?)
            .object("ChapterNames", self.chapter_names.build(lua)?)
            .object("PageContainerLinks", self.page_container_links.build(lua)?)
            .object("FileNames", self.file_names.build(lua)?)
            .integer_property("CurrentDownloadChapterPtr", |f: &mut TaskFields| {
                &mut f.current_download_chapter_ptr
            })
            .integer_property("PageNumber", |f: &mut TaskFields| &mut f.page_number)
            .integer_property("CurrentMaxFileNameLength", |f: &mut TaskFields| {
                &mut f.current_max_file_name_length
            })
            .string_property("Link", |f: &mut TaskFields| &mut f.link)
            .build(lua)?;
        Ok(object)
    }

    fn read(&self) -> Task {
        let f = self.fields.borrow();
        Task {
            page_links: read_strings(&self.page_links),
            page_container_links: read_strings(&self.page_container_links),
            file_names: read_strings(&self.file_names),
            chapter_links: read_strings(&self.chapter_links),
            chapter_names: read_strings(&self.chapter_names),
            current_download_chapter_ptr: f.current_download_chapter_ptr,
            page_number: f.page_number,
            current_max_file_name_length: f.current_max_file_name_length,
            link: text(&f.link),
        }
    }
}
