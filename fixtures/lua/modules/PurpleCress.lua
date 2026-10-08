----------------------------------------------------------------------------------------------------
-- Module Initialization
----------------------------------------------------------------------------------------------------

function Init()
	local m = NewWebsiteModule()
	m.ID                       = 'dc0ab1cf47c24c69aea536828fbbc1ca'
	m.Name                     = 'PurpleCress'
	m.RootURL                  = 'https://purplecress.com'
	m.Category                 = 'English-Scanlation'
	m.OnGetNameAndLink         = 'GetNameAndLink'
	m.OnGetInfo                = 'GetInfo'
	m.OnGetPageNumber          = 'GetPageNumber'
end

----------------------------------------------------------------------------------------------------
-- Event Functions
----------------------------------------------------------------------------------------------------

-- Get links and names from the manga list of the current website.
function GetNameAndLink()
	if not HTTP.GET(MODULE.RootURL) then return net_problem end

	local x = CreateTXQuery(HTTP.Document)
	local v for v in x.XPath('//div[@class="container-grid--small"]/a').Get() do
		LINKS.Add(v.GetAttribute('href'))
		NAMES.Add(x.XPathString('div/h3', v))
	end

	return no_error
end

-- Get info and chapter list for current manga.
function GetInfo()
	if not HTTP.GET(MaybeFillHost(MODULE.RootURL, URL)) then return net_problem end

	local x = CreateTXQuery(HTTP.Document)
	MANGAINFO.Title     = x.XPathString('//h1[@class="series__name"]')
	MANGAINFO.CoverLink = x.XPathString('//img[contains(@class, "series__img")]/@src')
	MANGAINFO.Authors   = x.XPathString('//p[@class="series__author"]/substring-after(.,"by ")')
	MANGAINFO.Genres    = x.XPathStringAll('//span[@class="series__tag"]')
	MANGAINFO.Status    = MangaInfoStatusIfPos(x.XPathString('//span[contains(@class, "series__status")]'))
	MANGAINFO.Summary   = x.XPathStringAll('//p[@class="description-pagagraph"]')

	local v for v in x.XPath('//div[@class="chapters__container"]/a').Get() do
		MANGAINFO.ChapterLinks.Add(v.GetAttribute('href'))
		MANGAINFO.ChapterNames.Add(x.XPathString('div/span', v))
	end
	MANGAINFO.ChapterLinks.Reverse(); MANGAINFO.ChapterNames.Reverse()

	return no_error
end

-- Get the page count for the current chapter.
function GetPageNumber()
	if not HTTP.GET(MaybeFillHost(MODULE.RootURL, URL)) then return net_problem end

	CreateTXQuery(HTTP.Document).XPathStringAll('//div[@class="pages__container"]//img/@src', TASK.PageLinks)

	return no_error
end
