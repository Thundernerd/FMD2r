function getinfo()
	MANGAINFO.URL=MaybeFillHost(MODULE.RootURL, URL)
	if HTTP.GET(MANGAINFO.URL) then
		local x=CreateTXQuery(HTTP.Document)
		if MANGAINFO.Title == '' then
			MANGAINFO.Title = x.XPathString('//h1[@class="tentruyen"]')
		end
		MANGAINFO.CoverLink = MaybeFillHost(MODULE.RootURL, x.XPathString('//div[contains(@class, "wrapper_image")]/img/@src'))
		MANGAINFO.Status = MangaInfoStatusIfPos(x.XPathString('//div[contains(@class, "wrapper_info")]/p[contains(., "Trạng thái")]'), 'Đang tiến hành', 'Hoàn thành')
		MANGAINFO.Authors=x.XPathString('//div[contains(@class, "wrapper_info")]/p[contains(., "Tác giả")]/substring-after(.,":")')
		MANGAINFO.Genres=x.XPathStringAll('//div[contains(@class, "wrapper_info")]/p[contains(., "Thể loại")]/a')
		MANGAINFO.Summary=x.XPathString('//p[@id="tomtattruyen"]')
		x.XPathHREFAll('//div[@id="wrapper_listchap"]//section/div/a', MANGAINFO.ChapterLinks, MANGAINFO.ChapterNames)
		MANGAINFO.ChapterLinks.Reverse(); MANGAINFO.ChapterNames.Reverse()
		return no_error
	else
		return net_problem
	end
end

function getpagenumber()
	TASK.PageLinks.Clear()
	if HTTP.GET(MaybeFillHost(MODULE.RootURL, URL)) then
		local x=CreateTXQuery(HTTP.Document)
		x.XPathStringAll('//div[@id="content_chap"]/img/@src', TASK.PageLinks)
	else
		return false
	end
	return true
end

function getnameandlink()
	local s = MODULE.RootURL .. string.format('/danhsach/P%s/index.html?sort=1', (URL + 1))
	if HTTP.GET(s) then
		local x = CreateTXQuery(HTTP.Document)
		x.XPathHREFAll('//a[h5[@class="tentruyen_slide"]]', LINKS, NAMES)
		local maxp = -1
		local v = x.XPath('//ul[@class="pagination"]/li/a/@href')
		for i = 1, v.Count do
			local p = tonumber(string.match(v.Get(i).ToString(), '/P(%d+)/'))
			if (p ~= nil) and (p > maxp) then maxp = p; end
		end
		if maxp > 0 then UPDATELIST.CurrentDirectoryPageNumber = maxp; end
		return no_error
	else
		return net_problem
	end
end

function Init()
	local m=NewWebsiteModule()
	m.ID = 'fcdf58f44aa04c6f8250a84f99fd2c4c'
	m.Category='Vietnamese'
	m.Name='HamTruyen'
	m.RootURL='https://hamtruyen.com'
	m.LastUpdated='June 15, 2018'
	m.OnGetInfo='getinfo'
	m.OnGetPageNumber='getpagenumber'
	m.OnGetNameAndLink='getnameandlink'
end
