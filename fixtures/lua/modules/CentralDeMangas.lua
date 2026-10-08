function GetInfo()
	MANGAINFO.URL=MaybeFillHost(MODULE.RootURL,URL)
	if HTTP.GET(MANGAINFO.URL) then
		x=CreateTXQuery(HTTP.Document)
		MANGAINFO.Title=x.XPathString('//div[@id="main"]//h1')
		MANGAINFO.CoverLink=MaybeFillHost(MODULE.RootURL, x.XPathString('//div[@id="main"]//div[@class="description"]/img/@src'))
		MANGAINFO.Summary = x.XPathString('//div[@id="main"]//div[@class="description"]/p')
		MANGAINFO.Artists = x.XPathStringAll('//div[@id="main"]//div[@class="content" and contains(div[@class="header"], "Arte")]/div[@class="description"]/a')
		MANGAINFO.Authors = x.XPathStringAll('//div[@id="main"]//div[@class="content" and contains(div[@class="header"], "Autor")]/div[@class="description"]/a')
		MANGAINFO.Genres = x.XPathStringAll('//div[@id="main"]//div[@class="content" and contains(div[@class="header"], "Gênero")]/div[@class="description"]/a')
		MANGAINFO.Status = MangaInfoStatusIfPos(x.XPathString('//div[@id="main"]//div[@class="content" and contains(div[@class="header"], "Status")]/div[@class="description"]/a'), 'Em publicação', 'Completo')
		x.XPathHREFAll('//table/tbody/tr/td/a', MANGAINFO.ChapterLinks, MANGAINFO.ChapterNames)
		MANGAINFO.ChapterLinks.Reverse(); MANGAINFO.ChapterNames.Reverse()
		return no_error
	else
		return net_problem
	end
end

function GetPageNumber()
	TASK.PageLinks.Clear()
	TASK.PageNumber=0
	if HTTP.GET(MaybeFillHost(MODULE.RootURL,URL)) then
		local x=CreateTXQuery(HTTP.Document)
		local s = x.XPathString('//script[contains(., "urlSulfix")]')
		local suffix = GetBetween("var urlSulfix = '", "';", s)
		local pages = GetBetween("var pages =", ";", s)
		x.ParseHTML(pages)
		local v = x.XPath('json(*)()');
		for i = 1, v.Count do
			local v1=v.Get(i)
			TASK.PageLinks.Add(suffix .. v1.ToString() .. '.jpg')
		end
		return true
	else
		return false
	end
end

function BeforeDownloadImage()
	HTTP.Headers.Values['Referer'] = MODULE.RootURL
	return true
end

function GetNameAndLink()
	if HTTP.GET(MODULE.RootURL..'/titulos/filtro/*/p/'..(URL + 1)) then
		x=CreateTXQuery(HTTP.Document)
		x.XPathHREFAll('//div[contains(@class, "list")]/div[contains(@class, "item")]/div[@class="content"]//a', LINKS, NAMES)
		return no_error
	else
		return net_problem
	end
end

function getdirectorypagenumber()
	if HTTP.GET(MODULE.RootURL .. '/titulos') then
		x = CreateTXQuery(HTTP.Document)
		PAGENUMBER = tonumber(x.XPathString('(//div[contains(@class, "grid")]/div/a[contains(@class, "button")])[last()]')) or 1
		return no_error
	else
		return net_problem
	end
end

function Init()
	local m=NewWebsiteModule()
	m.ID                       = '77dc987eec4e4ef49caf1911ba67e472'
	m.Category                 = 'Portuguese'
	m.Name                     = 'CentralDeMangas'
	m.RootURL                  = 'http://centraldemangas.online'
	m.OnGetInfo                = 'GetInfo'
	m.OnGetPageNumber          = 'GetPageNumber'
	m.OnGetNameAndLink         = 'GetNameAndLink'
	m.OnGetDirectoryPageNumber = 'getdirectorypagenumber'
	m.OnBeforeDownloadImage    = 'BeforeDownloadImage'
end