----------------------------------------------------------------------------------------------------
-- Local Constants
----------------------------------------------------------------------------------------------------

DirectoryPagination = '/bbs/page.php?hid=manga_list&page='

----------------------------------------------------------------------------------------------------
-- Event Functions
----------------------------------------------------------------------------------------------------

-- Get info and chapter list for current manga.
function GetInfo()
	local v, x = nil
	local u = MaybeFillHost(MODULE.RootURL, URL)

	if not HTTP.GET(u) then return net_problem end

	x = CreateTXQuery(HTTP.Document)
	MANGAINFO.Title     = x.XPathString('//div[@class="information"]//div[@class="red title"]')
	MANGAINFO.CoverLink = x.XPathString('//div[@class="manga-thumbnail"]/@style'):match('background%-image:URL%((.-)%)')
	MANGAINFO.Authors   = x.XPathString('//div[@class="manga-thumbnail"]/a[@class="author"]')
	MANGAINFO.Genres    = x.XPathStringAll('//div[@class="information"]/div[@class="manga-tags"]/a')

	for v in x.XPath('//div[@class="chapter-list"]//a').Get() do
		MANGAINFO.ChapterNames.Add(x.XPathString('div/text()', v))
		MANGAINFO.ChapterLinks.Add(v.GetAttribute('href'))
	end
	MANGAINFO.ChapterLinks.Reverse(); MANGAINFO.ChapterNames.Reverse()

	return no_error
end

-- Get the page count of the manga list of the current website.
function GetDirectoryPageNumber()
	local u, x = MODULE.RootURL .. DirectoryPagination .. 0

	if not HTTP.GET(u) then return net_problem end

	PAGENUMBER = tonumber(CreateTXQuery(HTTP.Document).XPathString('//ul[@class="pagination"]//li[last()]/a/@href'):match('%((.-)%)'))

	return no_error
end

-- Get LINKS and NAMES from the manga list of the current website.
function GetNameAndLink()
	local x = nil
	local u = MODULE.RootURL .. DirectoryPagination .. (URL + 1)

	if not HTTP.GET(u) then return net_problem end

	x = CreateTXQuery(HTTP.Document)
	x.XPathHREFAll('//div[@class="manga-subject"]/a', LINKS, NAMES)

	return no_error
end

-- Get the page count for the current chapter.
function GetPageNumber()
	local x = nil
	local u = MaybeFillHost(MODULE.RootURL, URL)

	if not HTTP.GET(u) then return net_problem end

	x = CreateTXQuery(HTTP.Document)
	x.ParseHTML(Trim(GetBetween('img_list = ', ';', x.XPathString('*'))))
	x.XPathStringAll('json(*)()', TASK.PageLinks)

	return no_error
end

----------------------------------------------------------------------------------------------------
-- Module Initialization
----------------------------------------------------------------------------------------------------

function Init()
	local m = NewWebsiteModule()
	m.ID                       = 'e641d814220346cdb8e5ae4268365955'
	m.Name                     = 'Manamoa'
	m.RootURL                  = 'https://manamoa.net'
	m.Category                 = 'Raw'
	m.OnGetInfo                = 'GetInfo'
	m.OnGetDirectoryPageNumber = 'GetDirectoryPageNumber'
	m.OnGetNameAndLink         = 'GetNameAndLink'
	m.OnGetPageNumber          = 'GetPageNumber'
end
