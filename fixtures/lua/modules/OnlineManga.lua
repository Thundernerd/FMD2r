----------------------------------------------------------------------------------------------------
-- Module Initialization
----------------------------------------------------------------------------------------------------

function Init()
	local m = NewWebsiteModule()
	m.ID                       = '6101054ece4a4837b32ae158d9fc1175'
	m.Name                     = 'OnlineManga'
	m.RootURL                  = 'https://adult.onlinemanga.xyz'
	m.Category                 = 'English'
	m.OnGetNameAndLink         = 'GetNameAndLink'
	m.OnGetDirectoryPageNumber = 'GetDirectoryPageNumber'
	m.OnGetInfo                = 'GetInfo'
	m.OnGetPageNumber          = 'GetPageNumber'
end

----------------------------------------------------------------------------------------------------
-- Local Constants
----------------------------------------------------------------------------------------------------

DirectoryPagination = '/list?page='

----------------------------------------------------------------------------------------------------
-- Event Functions
----------------------------------------------------------------------------------------------------

-- Get links and names from the manga list of the current website.
function GetNameAndLink()
	if not HTTP.GET(MODULE.RootURL .. DirectoryPagination .. (URL + 1)) then return net_problem end

	local x = CreateTXQuery(HTTP.Document)
	local v for v in x.XPath('//h4[@class="display-10"]/a').Get() do
		LINKS.Add(v.GetAttribute('href'))
		NAMES.Add(x.XPathString('span', v))
	end

	return no_error
end

-- Get the page count of the manga list of the current website.
function GetDirectoryPageNumber()
	if not HTTP.GET(MODULE.RootURL .. DirectoryPagination .. 1) then return net_problem end

	PAGENUMBER = tonumber(CreateTXQuery(HTTP.Document).XPathString('//ul[contains(@class, "pagination")]/li[last()-1]/a')) or 1

	return no_error
end

-- Get info and chapter list for current manga.
function GetInfo()
	if not HTTP.GET(MaybeFillHost(MODULE.RootURL, URL)) then return net_problem end

	local x = CreateTXQuery(HTTP.Document)
	MANGAINFO.Title     = x.XPathString('//h1[@class="display-10"]/a/span')
	MANGAINFO.CoverLink = x.XPathString('//img[contains(@class, "img-thumbnail")]/@src')
	MANGAINFO.Authors   = x.XPathStringAll('//td[contains(., "Authors:")]/ul//a')
	MANGAINFO.Genres    = x.XPathStringAll('//span[@itemprop="genre"]')
	MANGAINFO.Status    = MangaInfoStatusIfPos(x.XPathString('//td[contains(., "Status:")]'))
	MANGAINFO.Summary   = x.XPathString('//div[@class="panel-story-info-description"]')
	if MANGAINFO.Summary == '' then MANGAINFO.Summary = x.XPathString('//div[@id="noidungm"]') end

	x.XPathHREFAll('//table[@id="chaptersTable"]//a', MANGAINFO.ChapterLinks, MANGAINFO.ChapterNames)

	return no_error
end

-- Get the page count for the current chapter.
function GetPageNumber()
	if not HTTP.GET(MaybeFillHost(MODULE.RootURL, URL)) then return net_problem end

	CreateTXQuery(HTTP.Document).XPathStringAll('//div[@class="panel-body"]//a/@href', TASK.PageLinks)

	return no_error
end
