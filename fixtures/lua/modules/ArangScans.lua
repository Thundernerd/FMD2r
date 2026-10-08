----------------------------------------------------------------------------------------------------
-- Module Initialization
----------------------------------------------------------------------------------------------------

function Init()
	local m = NewWebsiteModule()
	m.ID                       = '4fade5ed11284de1812860d22ad324bf'
	m.Name                     = 'ArangScans'
	m.RootURL                  = 'https://arangscans.com'
	m.Category                 = 'English-Scanlation'
	m.OnGetNameAndLink         = 'GetNameAndLink'
	m.OnGetInfo                = 'GetInfo'
	m.OnGetPageNumber          = 'GetPageNumber'
end

----------------------------------------------------------------------------------------------------
-- Local Constants
----------------------------------------------------------------------------------------------------

DirectoryPagination = '/titles'

----------------------------------------------------------------------------------------------------
-- Event Functions
----------------------------------------------------------------------------------------------------

-- Get links and names from the manga list of the current website.
function GetNameAndLink()
	if not HTTP.GET(MODULE.RootURL .. DirectoryPagination) then return net_problem end

	CreateTXQuery(HTTP.Document).XPathHREFAll('//div[@class="content"]/h4/a', LINKS, NAMES)

	return no_error
end

-- Get info and chapter list for current manga.
function GetInfo()
	if not HTTP.GET(MaybeFillHost(MODULE.RootURL, URL)) then return net_problem end

	local x = CreateTXQuery(HTTP.Document)
	MANGAINFO.Title     = x.XPathString('//meta[@property="og:title"]/@content')
	MANGAINFO.CoverLink = x.XPathString('//meta[@property="og:image"]/@content')
	MANGAINFO.Authors   = x.XPathString('//p[contains(., "Author")]/substring-after(., ":")')
	MANGAINFO.Artists   = x.XPathString('//p[contains(., "Artist")]/substring-after(., ":")')
	MANGAINFO.Genres    = x.XPathStringAll('//div[(./h5="Tags")]/div')
	MANGAINFO.Summary   = x.XPathString('//meta[@property="og:description"]/@content')

	x.XPathHREFAll('//div[@class="content"]/p/a[contains(@href, "/read")]', MANGAINFO.ChapterLinks, MANGAINFO.ChapterNames)
	MANGAINFO.ChapterLinks.Reverse(); MANGAINFO.ChapterNames.Reverse()

	return no_error
end

-- Get the page count for the current chapter.
function GetPageNumber()
	if not HTTP.GET(MaybeFillHost(MODULE.RootURL, URL:gsub('/read', '/json'))) then return net_problem end

	local x = CreateTXQuery(HTTP.Document)
	local v for v in x.XPath('json(*).pages()').Get() do
		TASK.PageLinks.Add(MaybeFillHost(MODULE.RootURL, x.XPathString('address', v)))
	end

	return no_error
end
