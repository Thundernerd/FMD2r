function getinfo()
	MANGAINFO.URL=MaybeFillHost(MODULE.RootURL, URL)
	if HTTP.GET(MANGAINFO.URL) then
		local x=CreateTXQuery(HTTP.Document)
		MANGAINFO.Title=x.XPathString('//h1')
		MANGAINFO.CoverLink=MaybeFillHost(MODULE.RootURL, x.XPathString('//div[@class="item"]/div[@class="image"]/img/@src'))
		MANGAINFO.Genres=x.XPathStringAll('//div[@class="extra"]/a')
		MANGAINFO.Summary=x.XPathString('//div[@id="description"]')
		local s = x.XPathString('//script[contains(., "mangaChapterCollection")]')
		local name = GetBetween('View.Manga(', ');', s)
		name = name:match('mangaName:%s*[\'"]([^\'"]+)[\'"]')
		s = GetBetween('MangaChapter(', ');', s)
		x.ParseHTML(s)
		local v = x.XPath('json(*)()')
		for i = 1, v.Count do
			local v1 = v.Get(i)
			s = string.format('/%s/%s/%s/1/', name, x.XPathString('volume', v1), x.XPathString('number', v1))
			MANGAINFO.ChapterLinks.Add(s)
			MANGAINFO.ChapterNames.Add(x.XPathString('title', v1))
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
		local s = x.XPathString('//script[contains(., "chapterRouter")]')
		s = GetBetween('Chapter(', ');', s)
		x.ParseHTML(s)
		local base = x.XPathString('json(*).srcBaseUrl')
		local v = x.XPath('json(*).pages/*/src')
		for i = 1,v.Count do
			local v1=v.Get(i)
			TASK.PageLinks.Add(base .. '/' .. v1.ToString())
		end
	else
		return false
	end
	return true
end

function getnameandlink()
	if tonumber(URL) < 0 then return no_error end
	if HTTP.GET(MODULE.RootURL .. '/genre/all/page/'..(URL + 1)) then
		local x = CreateTXQuery(HTTP.Document)
		local v = x.XPath('//div[@class="genres"]/a[@class="genre"]')
		for i=1,v.Count do
			local v1=v.Get(i)
			LINKS.Add(v1.GetAttribute('href'))
			NAMES.Add(x.XPathString('div/h2', v1))
		end
		UPDATELIST.CurrentDirectoryPageNumber = tonumber(x.XPathString('(//a[@class="ui button"])[last()]')) or 1
		return no_error
	else
		return net_problem
	end
end

function Init()
	local m = NewWebsiteModule()
	m.ID = '6d4e5440764f48b1ab6d26a9e1473130'
	m.Name = 'MangaOnlineBiz'
	m.RootURL = 'https://manga-online.biz'
	m.Category = 'Russian'
	m.LastUpdated='March 3, 2018'
	m.OnGetInfo='getinfo'
	m.OnGetPageNumber='getpagenumber'
	m.OnGetNameAndLink='getnameandlink'
end
