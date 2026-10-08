----------------------------------------------------------------------------------------------------
-- Module Initialization
----------------------------------------------------------------------------------------------------

function Init()
	local m = NewWebsiteModule()
	m.ID                       = '041471dc634f4b639097ae0fede4bf7f'
	m.Name                     = 'ComicExtra'
	m.RootURL                  = 'https://www.comicextra.me'
	m.Category                 = 'English'
	m.OnGetNameAndLink         = 'GetNameAndLink'
	m.OnGetInfo                = 'GetInfo'
	m.OnGetPageNumber          = 'GetPageNumber'
	m.TotalDirectory           = AlphaList:len()
end

----------------------------------------------------------------------------------------------------
-- Local Constants
----------------------------------------------------------------------------------------------------

AlphaList = '#abcdefghijklmnopqrstuvwxyz'
DirectoryPagination = '/comic-list/'

----------------------------------------------------------------------------------------------------
-- Event Functions
----------------------------------------------------------------------------------------------------

-- Get links and names from the manga list of the current website.
function GetNameAndLink()
	local s, i, x = nil
	if MODULE.CurrentDirectoryIndex == 0 then
		s = 'others'
	else
		i = MODULE.CurrentDirectoryIndex + 1
		s = AlphaList:sub(i, i)
	end
	local u = MODULE.RootURL .. DirectoryPagination .. s

	if not HTTP.GET(u) then return net_problem end

	x = CreateTXQuery(HTTP.Document)
	x.XPathHREFAll('//div[@class="hlb-t"]/a', LINKS, NAMES)
	UPDATELIST.CurrentDirectoryPageNumber = 1

	return no_error
end

-- Get info and chapter list for current manga.
function GetInfo()
	local p, pages, pg, spages, spg, v, x = nil
	local u = MaybeFillHost(MODULE.RootURL, URL)

	if not HTTP.GET(u) then return net_problem end

	x = CreateTXQuery(HTTP.Document)
	MANGAINFO.Title     = x.XPathString('//div[contains(@class, "movie-detail")]/h1')
	MANGAINFO.CoverLink = MaybeFillHost(MODULE.RootURL, x.XPathString('//div[@class="movie-l-img"]/img/@src'))
	MANGAINFO.Authors   = x.XPathString('//dt[contains(., "Author")]/following-sibling::dd[1]')
	MANGAINFO.Genres    = x.XPathStringAll('//dt[contains(., "Genre")]/following-sibling::dd/a')
	MANGAINFO.Status    = MangaInfoStatusIfPos(x.XPathString('//dt[contains(., "Status")]/following-sibling::dd'))
	MANGAINFO.Summary   = x.XPathString('//div[@id="film-content"]')

	spages = x.XPathString('//div[@class="general-nav"]/a[last()]/@href')
	spages = string.sub(spages, string.len(spages) - 1, string.len(spages))
	spages = spages:gsub('/', '')
	if spages == '' or spages == 'N/A' then
		spages = 1
	end
	pages = tonumber(spages)
	p = 1
	while p <= pages do
		if p >= 1 then
			if HTTP.GET(u .. '/' .. p) then
				x = CreateTXQuery(HTTP.Document)
			else
				break
			end
		end

		if p == pages then
			spg = x.XPathString('//div[@class="general-nav"]/a[last()]/@href')
			spg = string.sub(spg, string.len(spg) - 1, string.len(spg))
			spg = spg:gsub('/', '')
			if spg == '' or spg == 'N/A' then
				spg = 1
			end
			pg = tonumber(spg)
			if pg ~= '' then pages = pg end
		end
		for v in x.XPath('//tbody[@id="list"]/tr/td/a').Get() do
			MANGAINFO.ChapterLinks.Add(v.GetAttribute('href'))
			MANGAINFO.ChapterNames.Add(v.ToString())
		end
		p = p + 1
	end
	MANGAINFO.ChapterLinks.Reverse(); MANGAINFO.ChapterNames.Reverse()

	return no_error
end

-- Get the page count for the current chapter.
function GetPageNumber()
	local u = MaybeFillHost(MODULE.RootURL, URL .. '/full')
	if not HTTP.GET(u) then return net_problem end

	CreateTXQuery(HTTP.Document).XPathStringAll('//img[@class="chapter_img"]/@src', TASK.PageLinks)

	return no_error
end
