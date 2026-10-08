function getinfo()
	MANGAINFO.URL = MaybeFillHost(MODULE.RootURL, URL)
	if HTTP.GET(MANGAINFO.URL) then
		x = CreateTXQuery(HTTP.Document)
		MANGAINFO.CoverLink = MaybeFillHost(MODULE.RootURL, x.XPathString('//div[@class="boxed"]/img/@src'))
		if MANGAINFO.Title == '' then
			MANGAINFO.Title = x.XPathString('//h1[contains(@class,"widget-title")]')
		end
		MANGAINFO.Status = MangaInfoStatusIfPos(x.XPathString('//span[contains(., "Estado")]/span'), 'progres', 'final')
		MANGAINFO.Authors = x.XPathStringAll('//span[contains(., "Autor")]/span')
		MANGAINFO.Genres = x.XPathStringAll('//dd[@class="sintesis-cat"]/a')
		MANGAINFO.Summary = x.XPathString('//div[@class="well"]/p')
		v = x.XPath('//ul/li/*[self::h5 or self::h3 and contains(@class, "chapter-title")]')
		for i = 1, v.Count do
			v2 = v.Get(i)
			MANGAINFO.ChapterLinks.Add(x.XPathString('a/@href', v2))
			MANGAINFO.ChapterNames.Add(x.XPathString('a||": "||em', v2))
		end
		MANGAINFO.ChapterLinks.Reverse(); MANGAINFO.ChapterNames.Reverse()
		return no_error
	else
		return net_problem
	end
end

function getpagenumber()
	TASK.PageLinks.Clear()
	if HTTP.GET(MaybeFillHost(MODULE.RootURL, URL)) then
		x = CreateTXQuery(HTTP.Document)
		s = x.XPathString('//script[contains(., "var pages")]')
		s = GetBetween('pages =', ';', s)
		x.ParseHTML(s)
		x.XPathStringAll('json(*)().page_image', TASK.PageLinks)
	else
		return false
	end
	return true
end

function getnameandlink()
	if HTTP.GET(MODULE.RootURL .. '/mangas') then
		local x = CreateTXQuery(HTTP.Document)
		x.XPathHREFAll('//div[@class="caption"]/h3/a', LINKS, NAMES)
		return no_error
	else
		return net_problem
	end
end

function Init()
	local m = NewWebsiteModule()
	m.ID = 'be9357b676df4dcabd34d7fc503d47c4'
	m.Category = 'Spanish'
	m.Name = 'MangaHispano'
	m.RootURL = 'https://mangahis.com'
	m.LastUpdated='June 14, 2018'
	m.OnGetInfo='getinfo'
	m.OnGetPageNumber='getpagenumber'
	m.OnGetNameAndLink='getnameandlink'
end