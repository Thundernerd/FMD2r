----------------------------------------------------------------------------------------------------
-- Module Initialization
----------------------------------------------------------------------------------------------------

function Init()
	local m = NewWebsiteModule()
	m.ID                       = '3895b697a2fe4abb9ae3dcb88258496b'
	m.Name                     = 'MangaSaki'
	m.RootURL                  = 'https://www.mangasaki.com'
	m.Category                 = 'English'
	m.OnGetDirectoryPageNumber = 'GetDirectoryPageNumber'
	m.OnGetNameAndLink         = 'GetNameAndLink'
	m.OnGetInfo                = 'GetInfo'
	m.OnGetPageNumber          = 'GetPageNumber'
end

----------------------------------------------------------------------------------------------------
-- Local Constants
----------------------------------------------------------------------------------------------------

DirectoryPagination = '/directory?page='

----------------------------------------------------------------------------------------------------
-- Event Functions
----------------------------------------------------------------------------------------------------

-- Get the page count of the manga list of the current website.
function GetDirectoryPageNumber()
	if not HTTP.GET(MODULE.RootURL .. DirectoryPagination .. 1) then return net_problem end

	PAGENUMBER = tonumber(CreateTXQuery(HTTP.Document).XPathString('//ul[contains(@class, "paginator")]/li[last()]/a/@href'):match('=(%d+)$')) or 1

  return no_error
end

-- Get links and names from the manga list of the current website.
function GetNameAndLink()
	if not HTTP.GET(MODULE.RootURL .. DirectoryPagination .. (URL + 1)) then return net_problem end

	CreateTXQuery(HTTP.Document).XPathHREFAll('//table[contains(@class, "directory_list")]/tbody//a', LINKS, NAMES)

	return no_error
end

-- Get info and chapter list for current manga.
function GetInfo()
	if not HTTP.GET(MaybeFillHost(MODULE.RootURL, URL)) then return net_problem end

	local x = CreateTXQuery(HTTP.Document)
	MANGAINFO.Title     = x.XPathString('//h1[@class="title"]')
	MANGAINFO.CoverLink = x.XPathString('//div[@class="content"]/div[contains(@class, "image2")]//img/@src')
	MANGAINFO.Authors   = x.XPathString('//div[@class="content"]/div[contains(@class, "author")]/div/div')
	MANGAINFO.Artists   = x.XPathString('//div[@class="content"]/div[contains(@class, "artist")]/div/div')
	MANGAINFO.Genres    = x.XPathStringAll('//div[@class="content"]/div[contains(@class, "genres")]//a')
	MANGAINFO.Status    = MangaInfoStatusIfPos(x.XPathString('//div[@class="content"]/div[contains(@class, "status")]/div/div'))
	MANGAINFO.Summary   = x.XPathString('//div[contains(@class, "summary")]//p')

	x.XPathHREFAll('//table[contains(@class, "chlist")]/tbody//a', MANGAINFO.ChapterLinks, MANGAINFO.ChapterNames)
	MANGAINFO.ChapterLinks.Reverse(); MANGAINFO.ChapterNames.Reverse()

	return no_error
end

-- Get the page count for the current chapter.
function GetPageNumber()
	if not HTTP.GET(MaybeFillHost(MODULE.RootURL, URL)) then return net_problem end

	local x = CreateTXQuery(HTTP.Document)
	x.ParseHTML(GetBetween('settings, ', ');', x.XPathString('//script[contains(., "showmanga")]')))
	x.XPathStringAll('json(*).showmanga.paths()', TASK.PageLinks)

	return no_error
end
