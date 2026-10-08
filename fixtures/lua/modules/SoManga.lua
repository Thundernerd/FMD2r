function getinfo()
	MANGAINFO.URL=MaybeFillHost(MODULE.RootURL, URL)
	if HTTP.GET(MANGAINFO.URL) then
		local x=CreateTXQuery(HTTP.Document)
		MANGAINFO.Title=x.XPathString('//h1')
		MANGAINFO.CoverLink=MaybeFillHost(MODULE.RootURL, x.XPathString('//div[contains(@class, "manga")]//img/@src'))
		MANGAINFO.Authors=x.XPathStringAll('//div[contains(@class, "manga")]//h5[contains(*, "Autor")]/text()', '')
		MANGAINFO.Artists=x.XPathStringAll('//div[contains(@class, "manga")]//h5[contains(*, "Artista")]/text()', '')
		MANGAINFO.Genres=x.XPathStringAll('//div[contains(@class, "manga")]//h5[contains(*, "Genero")]/span')
		MANGAINFO.Status = MangaInfoStatusIfPos(x.XPathString('//div[contains(@class, "manga")]//h5[contains(*, "Status")]/text()'), 'Ativo', 'Completo')
		MANGAINFO.Summary=x.XPathString('//div[contains(@class, "manga")]//div[contains(@style, "justify")]')
		local v=x.XPath('//ul[@class="capitulos"]/li/a')
		for i=1,v.Count do
			local v1=v.Get(i)
			MANGAINFO.ChapterLinks.Add(v1.GetAttribute('href'))
			MANGAINFO.ChapterNames.Add(x.XPathString('div/text()', v1))
		end
		MANGAINFO.ChapterLinks.Reverse(); MANGAINFO.ChapterNames.Reverse()
		return no_error
	else
		return net_problem
	end
end

function getpagenumber()
	TASK.PageLinks.Clear()
	TASK.PageNumber=0
	if HTTP.GET(MaybeFillHost(MODULE.RootURL, URL)) then
		local x=CreateTXQuery(HTTP.Document)
		x.XPathStringAll('//img[contains(@class, "img-manga")]/@src', TASK.PageLinks)
	else
		return false
	end
	return true
end

function getnameandlink()
	if HTTP.GET(MODULE.RootURL .. '/php/busca-mangas.php') then
		local x = CreateTXQuery(HTTP.Document)
		local v = x.XPath('//ul/li[@class="mangas"]/div/a', LINKS, NAMES)
		for i=1,v.Count do
			local v1=v.Get(i)
			LINKS.Add(v1.GetAttribute('href'))
			NAMES.Add(x.XPathString('div/h3', v1))
		end
		return no_error
	else
		return net_problem
	end
end

function Init()
	local m = NewWebsiteModule()
	m.ID = 'f153e850b9a14d6d81dd708ad26f77b0'
	m.Name = 'SoManga'
	m.RootURL = 'http://somangas.net'
	m.Category = 'Portuguese'
	m.LastUpdated='March 1, 2018'
	m.OnGetInfo='getinfo'
	m.OnGetPageNumber='getpagenumber'
	m.OnGetNameAndLink='getnameandlink'
end
