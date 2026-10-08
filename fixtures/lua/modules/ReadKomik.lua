----------------------------------------------------------------------------------------------------
-- Local Constants
----------------------------------------------------------------------------------------------------

DirectoryPagination = '/p/blog-page.html'

----------------------------------------------------------------------------------------------------
-- Event Functions
----------------------------------------------------------------------------------------------------

-- Get info and chapter list for current manga.
function GetInfo()
	local x = nil
	local u = MaybeFillHost(MODULE.RootURL, URL)

	if not HTTP.GET(u) then return net_problem end

	x = CreateTXQuery(HTTP.Document)
	MANGAINFO.Title     = Trim(x.XPathString('//title/substring-before(., " - ReadKomik")'))
	if MANGAINFO.Title  == "" then MANGAINFO.Title = Trim(x.XPathString('//title/substring-after(., "ReadKomik: ")')) end
	MANGAINFO.CoverLink = x.XPathString('//div[@class="kerangka-gambar"]//a/@href')
	MANGAINFO.Status    = MangaInfoStatusIfPos(x.XPathString('//td[contains(., "Status")]/following-sibling::td'))
	MANGAINFO.Genres    = x.XPathString('//td[contains(., "Genre")]/following-sibling::td')
	MANGAINFO.Summary   = x.XPathString('//div[@class="isi-sinopsis"]')

	for v in x.XPath('//*[@dir="ltr" or @class="tabel-chapter"]/a').Get() do
		MANGAINFO.ChapterNames.Add(x.XPathString('span', v))
		MANGAINFO.ChapterLinks.Add(v.GetAttribute('href'))
	end
	if MANGAINFO.ChapterLinks.Count < 1 then
		x.XPathHREFAll('//div[contains(@itemprop, "description")]/a', MANGAINFO.ChapterLinks, MANGAINFO.ChapterNames)
	end
	if MANGAINFO.ChapterLinks.Count < 1 then
		x.XPathHREFAll('//h2[@class="entry-title"]/a', MANGAINFO.ChapterLinks, MANGAINFO.ChapterNames)
	end

	return no_error
end

-- Get LINKS and NAMES from the manga list of the current website.
function GetNameAndLink()
	local x = nil
	local u = MODULE.RootURL .. DirectoryPagination

	if not HTTP.GET(u) then return net_problem end

	x = CreateTXQuery(HTTP.Document)
	x.XPathHREFAll('//div[contains(@itemprop, "description")]/a', LINKS, NAMES)

	return no_error
end

-- Get the page count for the current chapter.
function GetPageNumber()
	local x = nil
	local u = MaybeFillHost(MODULE.RootURL, URL)

	if not HTTP.GET(u) then return net_problem end

	x = CreateTXQuery(HTTP.Document)
	x.XPathStringAll('//div[@class="separator"]/a/@href', TASK.PageLinks)

	return no_error
end

----------------------------------------------------------------------------------------------------
-- Module Initialization
----------------------------------------------------------------------------------------------------

function Init()
	local m = NewWebsiteModule()
	m.ID                       = 'f19d56bb21ea46bea8bee5b9c083d4d0'
	m.Name                     = 'ReadKomik'
	m.RootURL                  = 'https://www.readkomik.com'
	m.Category                 = 'Webcomics'
	m.OnGetInfo                = 'GetInfo'
	m.OnGetNameAndLink         = 'GetNameAndLink'
	m.OnGetPageNumber          = 'GetPageNumber'
end
