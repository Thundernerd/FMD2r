----------------------------------------------------------------------------------------------------
-- Module Initialization
----------------------------------------------------------------------------------------------------

function Init()
	local m = NewWebsiteModule()
	m.ID                       = '206c360cb8bc45799a475e116e96ba53'
	m.Name                     = 'GrafiManga'
	m.RootURL                  = 'https://grafimanga.com'
	m.Category                 = 'Spanish-Scanlation'
	m.OnGetNameAndLink         = 'GetNameAndLink'
	m.OnGetInfo                = 'GetInfo'
	m.OnGetPageNumber          = 'GetPageNumber'
end

----------------------------------------------------------------------------------------------------
-- Local Constants
----------------------------------------------------------------------------------------------------

local Template = require 'templates.MangaReaderOnline'
-- DirectoryParameters = '/'            --> Override template variable by uncommenting this line.

----------------------------------------------------------------------------------------------------
-- Event Functions
----------------------------------------------------------------------------------------------------

-- Get links and names from the manga list of the current website.
function GetNameAndLink()
	Template.GetNameAndLink()

	return no_error
end

-- Get info and chapter list for current manga.
function GetInfo()
	local v, x = nil
	local u = MaybeFillHost(MODULE.RootURL, URL)

	if not HTTP.GET(u) then return net_problem end

	x = CreateTXQuery(HTTP.Document)
	MANGAINFO.Title     = x.XPathString('//h1[@class="manga-title"]')
	MANGAINFO.CoverLink = x.XPathString('//div[@class="show-boxed"]/img/@src')
	MANGAINFO.Status    = MangaInfoStatusIfPos(x.XPathString('//div[@class="spec"]/span[2]'))
	MANGAINFO.Genres    = x.XPathStringAll('//div[@class="manga-cat"]/a')
	MANGAINFO.Summary   = x.XPathString('//div[@class="manga-desc"]/p')

	for v in x.XPath('//div[@class="chapters"]//div[@class="chapters-content"]').Get() do
		MANGAINFO.ChapterLinks.Add(x.XPathString('div/h3/a/@href', v))
		MANGAINFO.ChapterNames.Add(x.XPathString('div[1]', v))
	end
	MANGAINFO.ChapterLinks.Reverse(); MANGAINFO.ChapterNames.Reverse()

	return no_error
end

-- Get the page count for the current chapter.
function GetPageNumber()
	Template.GetPageNumber()

	return no_error
end
