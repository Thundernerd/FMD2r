----------------------------------------------------------------------------------------------------
-- Module Initialization
----------------------------------------------------------------------------------------------------

function Init()
	local m = NewWebsiteModule()
	m.ID                       = '47a611b9efb44e34a800bdb0f946ff07'
	m.Name                     = 'MangaHost'
	m.RootURL                  = 'https://mangahost4.com'
	m.Category                 = 'Portuguese'
	m.OnGetDirectoryPageNumber = 'GetDirectoryPageNumber'
	m.OnGetNameAndLink         = 'GetNameAndLink'
	m.OnGetInfo                = 'GetInfo'
	m.OnGetPageNumber          = 'GetPageNumber'
end

----------------------------------------------------------------------------------------------------
-- Local Constants
----------------------------------------------------------------------------------------------------

local DirectoryPagination = '/mangas/page/'

----------------------------------------------------------------------------------------------------
-- Event Functions
----------------------------------------------------------------------------------------------------

-- Get the page count of the manga list of the current website.
function GetDirectoryPageNumber()
	local u = MODULE.RootURL .. DirectoryPagination .. 1

	if not HTTP.GET(u) then return net_problem end

	PAGENUMBER = tonumber(CreateTXQuery(HTTP.Document).XPathString('(//div[@class="wp-pagenavi"])[1]//a[@class="last"]/@href'):match('.-/page/(%d+)'))

	return no_error
end

-- Get links and names from the manga list of the current website.
function GetNameAndLink()
	local x = nil
	local u = MODULE.RootURL .. DirectoryPagination .. (URL + 1)

	if not HTTP.GET(u) then return net_problem end

	x = CreateTXQuery(HTTP.Document)
	x.XPathHREFAll('//*[@class="manga-block-title"]/a', LINKS, NAMES)

	return no_error
end

-- Get info and chapter list for current manga.
function GetInfo()
	local v, x = nil
	local u = MaybeFillHost(MODULE.RootURL, URL)

	if not HTTP.GET(u) then return net_problem end

	x = CreateTXQuery(HTTP.Document)
	MANGAINFO.Title     = x.XPathString('//meta[@property="og:title"]/@content')
	MANGAINFO.CoverLink = x.XPathString('//div[@class="widget"]/picture/img/@src')
	MANGAINFO.Status    = MangaInfoStatusIfPos(x.XPathString('//div[contains(strong, "Status:")]/text()'), 'Ativo', 'Completo')
	MANGAINFO.Authors   = x.XPathString('//div[contains(strong, "Autor:")]/text()')
	MANGAINFO.Artists   = x.XPathString('//div[contains(strong, "Arte:")]/text()')
	MANGAINFO.Genres    = x.XPathStringAll('//div[@class="w-col w-col-7"]/article/div[@class="tags"]/a')
	MANGAINFO.Summary   = Trim(x.XPathString('//div[@class="paragraph"]'))

	for v in x.XPath('//div[@class="card pop"]').Get() do
		MANGAINFO.ChapterLinks.Add(x.XPathString('div[2]/div/a/@href', v))
		MANGAINFO.ChapterNames.Add(x.XPathString('div[1]/text()', v) .. ' ' .. x.XPathString('div[1]/span/text()', v))
	end
	MANGAINFO.ChapterLinks.Reverse(); MANGAINFO.ChapterNames.Reverse()

	return no_error
end

-- Get the page count for the current chapter.
function GetPageNumber()
	local x = nil
	local u = MaybeFillHost(MODULE.RootURL, URL)

	if not HTTP.GET(u) then return net_problem end

	x = CreateTXQuery(HTTP.Document)
	x.ParseHTML(GetBetween('var images = ["', '"];', x.XPathString('//script[contains(., "var images = ")]')):gsub('"', ''):gsub('\'', '"'))
	x.XPathStringAll('//img/@src', TASK.PageLinks)

	return no_error
end
