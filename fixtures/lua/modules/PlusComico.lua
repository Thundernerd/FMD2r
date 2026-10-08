function getinfo_store()
	local x=CreateTXQuery(HTTP.Document)
	MANGAINFO.Title=x.XPathString('//div[@class="_title"]')
	MANGAINFO.CoverLink=MaybeFillHost(MODULE.RootURL, x.XPathString('//meta[@property="og:image"]/@content'))
	MANGAINFO.Authors=x.XPathString('//*[contains(@class,"__author")]')
	MANGAINFO.Genres=x.XPathStringAll('//li[contains(@class,"__list-genre-item")]/p/a')
	MANGAINFO.Summary=x.XPathString('//p[@class="_description"]')
	local id = MANGAINFO.URL:match('/(%d+)/?$')
	HTTP.Reset()
	if HTTP.POST(MODULE.RootURL .. '/store/api/getTitleArticles.nhn', 'titleNo='..id) then
		x.ParseHTML(HTTP.Document)
		local v = x.XPath('json(*).result.list().articleList()')
		for i = 1, v.Count do
			local v1 = v.Get(i)
			if x.XPathString('./freeFlg', v1) == 'Y' then
				MANGAINFO.ChapterLinks.Add(x.XPathString('./articleDetailUrl', v1))
				MANGAINFO.ChapterNames.Add(x.XPathString('./subtitle', v1))
			end
		end
	end
end

function getinfo_manga()
	local x=CreateTXQuery(HTTP.Document)
	MANGAINFO.Title=x.XPathString('//*[contains(@class,"__ttl")]')
	MANGAINFO.CoverLink=MaybeFillHost(MODULE.RootURL, x.XPathString('//meta[@property="og:image"]/@content'))
	MANGAINFO.Authors=x.XPathString('//*[contains(@class,"__author")]')
	MANGAINFO.Genres=x.XPathStringAll('//*[contains(@class,"__meta")]//a')
	MANGAINFO.Summary=x.XPathString('//*[contains(@class,"__description")]')
	local id = MANGAINFO.URL:match('/(%d+)/?$')
	HTTP.Reset()
	if HTTP.POST(MODULE.RootURL .. '/api/getArticleList.nhn', 'titleNo='..id) then
		x.ParseHTML(HTTP.Document)
		local v = x.XPath('json(*).result.list()')
		for i = 1, v.Count do
			local v1 = v.Get(i)
			if x.XPathString('./freeFlg', v1) == 'Y' then
				MANGAINFO.ChapterLinks.Add(x.XPathString('./articleDetailUrl', v1))
				MANGAINFO.ChapterNames.Add(x.XPathString('./subtitle', v1))
			end
		end
	end
end

function getinfo()
	MANGAINFO.URL=MaybeFillHost(MODULE.RootURL, URL)
	if HTTP.GET(MANGAINFO.URL) then
		if MANGAINFO.URL:match('/store/') ~= nil then
			getinfo_store()
		else
			getinfo_manga()
		end
		return no_error
	else
		return net_problem
	end
end

function getpagenumber_manga()
	local x=CreateTXQuery(HTTP.Document)
	local s = x.XPathString('//script[contains(., "imageData:")]')
	s = GetBetween('imageData:', ']', s) .. ']'
	x.ParseHTML(s)
	x.XPathStringAll('json(*)()', TASK.PageLinks)
	return true
end

function getpagenumber_store()
	if HTTP.LastURL:match('param=') == nil then return false; end
	local base = SeparateLeft(HTTP.LastURL, 'index.php')
	local param = GetBetween('param=', '&', HTTP.LastURL)
	local ts = os.time() * 1000
	base = base .. string.format('diazepam_hybrid.php?reqtype=0&param=%s&ts=%d&_=%d', param, ts, ts+1500)
	math.randomseed(os.time())
	math.random(); math.random(); math.random();
	local tpurl = string.format('&mode=7&file=face.xml&callback=jQ%d_%d', math.random(1000), math.random(1000))
	if HTTP.GET(base .. tpurl) then
		if HTTP.Terminated then return false; end
		local s = GetBetween('("', '")', HTTP.Document.ToString())
		local x=CreateTXQuery(s)
		local total_pages = tonumber(x.XPathString('//TotalPage'))
		if total_pages == nil then return false; end
		for i = 0, total_pages-1 do
			TASK.PageLinks.Add(base .. string.format('&mode=8&file=%04d.xml', i))
		end
		TASK.PageContainerLinks.Text = HTTP.Cookies.Text
	else
		return false
	end
	return false
end

function getpagenumber()
	TASK.PageLinks.Clear()
	if HTTP.GET(MaybeFillHost(MODULE.RootURL, URL)) then
		if URL:match('/store/') ~= nil then
			return getpagenumber_store()
		else
			return getpagenumber_manga()
		end
	else
		return false
	end
end

local js = require 'utils.jsunpack'
function downloadimage()
	if URL:match('%.xml') == nil then return HTTP.GET(URL); end
	HTTP.Cookies.Text = TASK.PageContainerLinks.Text
	if HTTP.GET(URL) then
		local x = CreateTXQuery(HTTP.Document)
		local a = js.splitstr(x.XPathString('//Scramble'), ',')
		local imgurl = SeparateLeft(URL, '&mode=')
		imgurl = imgurl .. string.format('&mode=1&file=%04d_0000.bin', WORKID)
		if HTTP.GET(imgurl) then
			local s = require('fmd.imagepuzzle').Create(4, 4)
			s.multiply = 8
			local n = 0
			for i, v in ipairs(a) do
				local j = tonumber(v)
				if j == nil then j = 0 end
				s.matrix[j] = n;
				n = n + 1
			end
			s.descramble(HTTP.Document, HTTP.Document)
			return true
		end
		return false
	end
	return false
end

local dirurls = {
--  '/store/ranking/list.nhn',
	'/manga/ranking/list.nhn'
}

function getnameandlink()
	local lurl = dirurls[MODULE.CurrentDirectoryIndex+1]
	local data = 'page='..(URL + 1)..'&rankingType=original'
	if HTTP.POST(MODULE.RootURL .. lurl, data) then
		local x = CreateTXQuery(HTTP.Document)
		local total = tonumber(x.XPathString('json(*).result.totalPageCnt'))
		if total == nil then total = 1 end
		UPDATELIST.CurrentDirectoryPageNumber = total
		local v = x.XPath('json(*).result.list()')
		for i = 1, v.Count do
			local v1 = v.Get(i)
			LINKS.Add(x.XPathString('title_url', v1))
			NAMES.Add(x.XPathString('title_name', v1))
		end
		return no_error
	else
		return net_problem
	end
end

function Init()
	local m = NewWebsiteModule()
	m.ID = '1505b134512d4c2aa0a1ba8e1ceaff3f'
	m.Name = 'PlusComico'
	m.RootURL = 'http://plus.comico.jp'
	m.Category = 'Raw'
	m.LastUpdated='May 1, 2018'
	m.OnGetInfo='getinfo'
	m.OnGetPageNumber='getpagenumber'
	m.OnGetNameAndLink='getnameandlink'
	m.TotalDirectory = #dirurls
	m.OnDownloadImage = 'downloadimage'
end
