function Init()
	local m = NewWebsiteModule()
	m.ID                         = '507f953835e742b5bde5c8a77a360d3c'
	m.Name                       = 'SenManga'
	m.RootURL                    = 'https://www.senmanga.com'
	m.Category                   = 'English'
	m.OnGetDirectoryPageNumber   = 'GetDirectoryPageNumber'
	m.OnGetNameAndLink           = 'GetNameAndLink'
	m.OnGetInfo                  = 'GetInfo'
	m.OnGetPageNumber            = 'GetPageNumber'
	m.MaxTaskLimit               = 1
	m.MaxConnectionLimit         = 4
end

function GetDirectoryPageNumber()
	if HTTP.GET(MODULE.RootURL .. '/directory') then
		PAGENUMBER = tonumber(CreateTXQuery(HTTP.Document).x.XPathString('//ul[@class="pagination"]/li[./a/@rel="next"]/preceding-sibling::li[1]')) or 1
		return no_error
	else
		return net_problem
	end
end

function GetNameAndLink()
	if HTTP.GET(MODULE.RootURL .. '/directory?page=' .. (URL + 1)) then
		CreateTXQuery(HTTP.Document).XPathHREFAll('//div[@class="details"]/p[@class="title"]/a', LINKS, NAMES)
		return no_error
	else
		return net_problem
	end
end

function GetInfo()
	MANGAINFO.URL=MaybeFillHost(MODULE.RootURL,URL):gsub('/*$','')
	if HTTP.GET(MANGAINFO.URL) then
		x=CreateTXQuery(HTTP.Document)
		MANGAINFO.Title     = x.XPathString('//h1[@class="title"]')
		MANGAINFO.CoverLink = x.XPathString('//div[@class="thumbnail"]/img/@src')
		MANGAINFO.Authors   = Trim(x.XPathStringAll('//ul[@class="series-info"]/li[contains(.,"Author:")]/substring-after(text(),":")'))
		MANGAINFO.Artists   = Trim(x.XPathStringAll('//ul[@class="series-info"]/li[contains(.,"Artist:")]/substring-after(text(),":")'))
		MANGAINFO.Summary   = x.XPathString('//*[@itemprop="description"]');
		MANGAINFO.Genres    = x.XPathStringAll('//ul[@class="series-info"]/li[contains(., "Categories:")]/a')
		MANGAINFO.Status    = MangaInfoStatusIfPos((x.XPathString('//ul[@class="series-info"]/li[contains(.,"Status:")]/substring-after(text(),":")')))
		x.XPathHREFAll('//div[@class="title" and contains(., "Chapters")]/following-sibling::div/div/a', MANGAINFO.ChapterLinks, MANGAINFO.ChapterNames)
		MANGAINFO.ChapterLinks.Reverse(); MANGAINFO.ChapterNames.Reverse()
		return no_error
	else
		return net_problem
	end
end

function GetPageNumber()
	HTTP.Cookies.Values['viewer'] = '1'
	if HTTP.GET(MaybeFillHost(MODULE.RootURL, URL)) then
		local x = CreateTXQuery(HTTP.Document)
		local s = x.XPathString('//script[contains(., "var imglist")]')
		s = GetBetween('var imglist = ', ';', s):gsub('%s', '')
		x.ParseHTML(s)
		x.XPathStringAll('json(*)().URL', TASK.PageLinks)
		return true
	else
		return false
	end
end
