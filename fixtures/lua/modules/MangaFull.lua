function getinfo()
	MANGAINFO.URL=MaybeFillHost(MODULE.RootURL, URL)
	if HTTP.GET(MANGAINFO.URL) then
			local x=CreateTXQuery(HTTP.Document)
		MANGAINFO.Title = x.XPathString('//div[@class="media-body"]//h1')
		MANGAINFO.CoverLink=MaybeFillHost(MODULE.RootURL, x.XPathString('//div[@class="media-left cover-detail"]//img/@src'))
			MANGAINFO.Authors=x.XPathString('//div[@class="media-body"]/p/span[starts-with(.,"Author(s): ")]')
		MANGAINFO.Genres=x.XPathStringAll('//div[@class="media-body"]/p/span[starts-with(.,"Genre: ")]')
		MANGAINFO.Summary=x.XPathString('//div[@class="manga-content"]/p')
		MANGAINFO.Status=MangaInfoStatusIfPos(x.XPathString('//*[@class="description-update"]//span[starts-with(.,"Status")]'))
		local v=x.XPath('//div[@class="chapter-list"]/ul/li[@class="row"]//a')
		for i=1,v.Count do
		local v1=v.Get(i)
		MANGAINFO.ChapterLinks.Add(v1.GetAttribute('href'))
		MANGAINFO.ChapterNames.Add(v1.GetAttribute('title'))
			end
		MANGAINFO.ChapterLinks.Reverse(); MANGAINFO.ChapterNames.Reverse()
		return no_error
	else
		return net_problem
	end
end

function getpagenumber()
	TASK.PageLinks.Clear()
	if HTTP.GET(MaybeFillHost(MODULE.RootURL,URL .. '/0')) then
		 CreateTXQuery(HTTP.Document).XPathStringAll('//div[@class="each-page"]//img/@src', TASK.PageLinks)
		 return true
	end
	return false
end

function getnameandlink()
	if HTTP.GET(MODULE.RootURL..'/popular-manga?page='.. (URL + 1)) then
		local x=CreateTXQuery(HTTP.Document)
		local v = x.XPath('//*[@class="cate-manga"]//*[@class="media-body"]/a')
		for i = 1, v.Count do
			local v1 = v.Get(i)
				local title = v1.GetAttribute('title')
						title = string.gsub(title, 'Manga', '')
			NAMES.Add(title);
				LINKS.Add(v1.GetAttribute('href'));
		end
		--p = tonumber(500)
		p = tonumber(x.XPathString('//div[@class="pagination"]/ul/li[last()]/substring-after(@href, "?page=")'))
		if p ~= nil then
			UPDATELIST.CurrentDirectoryPageNumber = p
		end
		return no_error
	else
		return net_problem
	end
end

function Init()
	local m = NewWebsiteModule()
	m.ID = '005cde3366724628aef853fb9dd9e17b'
	m.Name = 'MangaFull'
	m.RootURL = 'https://mangafull.org'
	m.Category = 'English'
	m.LastUpdated='May 14, 2019'
	m.SortedList = true
	m.OnGetInfo='getinfo'
	m.OnGetPageNumber='getpagenumber'
	m.OnGetNameAndLink='getnameandlink'
end
