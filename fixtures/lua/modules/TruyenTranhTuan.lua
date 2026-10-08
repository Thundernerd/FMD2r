function getinfo()
	MANGAINFO.URL=MaybeFillHost(MODULE.RootURL, URL)
	if HTTP.GET(MANGAINFO.URL) then
		local x=CreateTXQuery(HTTP.Document)
		if MANGAINFO.Title == '' then
			MANGAINFO.Title = x.XPathString('//h1[@itemprop="name"]')
		end
		MANGAINFO.CoverLink = MaybeFillHost(MODULE.RootURL, x.XPathString('//*[@class="manga-cover"]/img/@src'))
		MANGAINFO.Status = MangaInfoStatusIfPos(x.XPathString('//li[contains(@class, "status")]/p[2]'), 'Đang tiến hành', 'Hoàn thành')
		MANGAINFO.Authors=x.XPathStringAll('//p[@class="misc-infor" and starts-with(.,"Tác giả")]/a')
		MANGAINFO.Genres=x.XPathStringAll('//p[@class="misc-infor" and starts-with(.,"Thể loại")]/a')
		MANGAINFO.Summary=x.XPathString('//*[@id="manga-summary"]/p')
		x.XPathHREFAll('//*[@id="manga-chapter"]//*[@class="chapter-name"]/a', MANGAINFO.ChapterLinks, MANGAINFO.ChapterNames)
		MANGAINFO.ChapterLinks.Reverse(); MANGAINFO.ChapterNames.Reverse()
		return no_error
	else
		return net_problem
	end
end

function getpagenumber()
	TASK.PageLinks.Clear()
	if HTTP.GET(MaybeFillHost(MODULE.RootURL, URL)) then
		local x = CreateTXQuery(HTTP.Document)
		local s = x.XPathString('//script[contains(.,"var slides_page")]')
		x.ParseHTML(GetBetween('slides_page_path = ', ';', s))
		local v = x.XPath('json(*)()')
		if v.Count > 0 then
			local tmp = {}
			for i = 1, v.Count do
				table.insert(tmp, v.Get(i).ToString())
			end
			table.sort(tmp)
			for i, link in ipairs(tmp) do
				TASK.PageLinks.Add(link)
			end
		else
			x.ParseHTML(GetBetween('slides_page_url_path = ', ';', s))
			local v = x.XPathStringAll('json(*)()', TASK.PageLinks)
		end
	else
		return false
	end
	return true
end

function getnameandlink()
	local s = MODULE.RootURL
	if URL ~= '0' then
		s = s .. '/page/' .. (URL + 1)
	end
	if HTTP.GET(s) then
		local x = CreateTXQuery(HTTP.Document)
		local p = 1
		local v = x.XPath('//*[@id="page-nav"]//li')
		for i = 1, v.Count do
			local tmp = tonumber(v.Get(i).ToString())
			if tmp == nil then tmp = 1; end
			if tmp > p then p = tmp; end
		end
		UPDATELIST.CurrentDirectoryPageNumber = p
		x.XPathHREFAll('//*[@id="story-list"]/div/span[1]/a', LINKS, NAMES)
		return no_error
	else
		return net_problem
	end
end

function Init()
	local m=NewWebsiteModule()
	m.ID = 'a4b6980afef4474f9cae83c975359cd0'
	m.Category='Vietnamese'
	m.Name='TruyenTranhTuan'
	m.RootURL='http://truyentranhtuan.com'
	m.LastUpdated='June 20, 2018'
	m.OnGetInfo='getinfo'
	m.OnGetPageNumber='getpagenumber'
	m.OnGetNameAndLink='getnameandlink'
end
