----------------------------------------------------------------------------------------------------
-- Local Constants
----------------------------------------------------------------------------------------------------

local DirectoryPagination = '/truyen/all?page='

----------------------------------------------------------------------------------------------------
-- Event Functions
----------------------------------------------------------------------------------------------------

-- Get info and chapter list for current manga.
function GetInfo()
	local v, x = nil
	local u = MaybeFillHost(MODULE.RootURL, URL)

	if not HTTP.GET(u) then return net_problem end

	x = CreateTXQuery(HTTP.Document)
	MANGAINFO.Title     = x.XPathString('//meta[@property="og:title"]/@content')
	MANGAINFO.CoverLink = x.XPathString('//meta[@property="og:image"]/@content')
	MANGAINFO.Status    = MangaInfoStatusIfPos(x.XPathString('//div[@class="__info"]/p[starts-with(.,"Tình trạng:")]'), 'Đang tiến hành', 'Ngưng')
	MANGAINFO.Authors   = SeparateRight(x.XPathString('//div[@class="__info"]/p[starts-with(.,"Tác giả:")]'), ':')
	MANGAINFO.Genres    = Trim(SeparateRight(x.XPathString('//div[@class="__info"]/p[starts-with(.,"Thể loại:")]'), ':'))
	MANGAINFO.Summary   = x.XPathString('//meta[@property="og:description"]/@content')

	x.XPathHREFAll('//div[@class="table-scroll"]/table/tbody/tr/td[1]/a', MANGAINFO.ChapterLinks, MANGAINFO.ChapterNames)
	MANGAINFO.ChapterLinks.Reverse(); MANGAINFO.ChapterNames.Reverse()

	return no_error
end

-- Get the page count of the manga list of the current website.
function GetDirectoryPageNumber()
	local u = MODULE.RootURL .. DirectoryPagination .. 1

	if not HTTP.GET(u) then return net_problem end

	PAGENUMBER = tonumber(CreateTXQuery(HTTP.Document).XPathString('(//*[starts-with(@class,"pagination")]//a)[last()-1]'))

	return no_error
end

-- Get LINKS and NAMES from the manga list of the current website.
function GetNameAndLink()
	local x = nil
	local u = MODULE.RootURL .. DirectoryPagination .. (URL + 1)

	if not HTTP.GET(u) then return net_problem end

	x = CreateTXQuery(HTTP.Document)
	x.XPathHREFAll('//table[1]/tbody/tr/td[1]/a', LINKS, NAMES)

	return no_error
end

-- Get the page count for the current chapter.
function GetPageNumber()
	local s, x = nil
	local u = MaybeFillHost(MODULE.RootURL, URL)

	if not HTTP.GET(u) then return net_problem end

	CreateTXQuery(HTTP.Document).XPathStringAll('//*[@class="manga-container"]/img/@src', TASK.PageLinks)

	return no_error
end

----------------------------------------------------------------------------------------------------
-- Module Initialization
----------------------------------------------------------------------------------------------------

function Init()
	local m = NewWebsiteModule()
	m.ID                       = '4269c1fceff2471d84d32ccfbfe2d541'
	m.Name                     = 'AcademyVN'
	m.RootURL                  = 'https://hocvientruyentranh.net'
	m.Category                 = 'Vietnamese'
	m.OnGetInfo                = 'GetInfo'
	m.OnGetDirectoryPageNumber = 'GetDirectoryPageNumber'
	m.OnGetNameAndLink         = 'GetNameAndLink'
	m.OnGetPageNumber          = 'GetPageNumber'
end