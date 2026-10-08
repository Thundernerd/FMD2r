function getinfo()
	MANGAINFO.URL=MaybeFillHost(MODULE.RootURL, URL)
	if HTTP.GET(MANGAINFO.URL) then
		local x=CreateTXQuery(HTTP.Document)
		if MODULE.Name == '11Toon' then
		MANGAINFO.Title=x.XPathString('//*[@class="title"]')
		MANGAINFO.CoverLink=x.XPathString('//*[@class="banner"]/@src')
		MANGAINFO.Genres=x.XPathString('//*[@class="genre-link"]/text()')
		local pages = 1
		local p = 1
		while p <= pages do

			if p > 1 then
			if HTTP.GET(MANGAINFO.URL .. '&sord=&type=&page=' .. tostring(p)) then
				x=CreateTXQuery(HTTP.Document)
			else
				break
			end
			end
			if p == pages then
			local pg = x.XPathString('//a[contains(@class, "pg_end")]/substring-after(@href, "&page=")')
			if pg ~= '' then pages = tonumber(pg) end
			end
			local v=x.XPath('//ul[@class="episode-list"]//li/button')
			for i=1,v.Count do
			local v1=v.Get(i)
			local link = v1.GetAttribute('onclick')
						link = link:gsub('location.href=', ''):gsub('./', '/'):gsub("'", '')
			MANGAINFO.ChapterLinks.Add('/bbs'..link)
			MANGAINFO.ChapterNames.Add(x.XPathString('.//*[@class="episode-title ellipsis"]/text()', v1))
			end
			p = p + 1
		end
	else
		if MANGAINFO.Title == '' then
			MANGAINFO.Title=x.XPathString('css("div.manga-subject")')
		end
		local img = x.XPathString('css("div.manga-thumbnail")/@style')
		img = GetBetween('URL(', ')', img)
		MANGAINFO.CoverLink=MaybeFillHost(MODULE.RootURL, img)
		MANGAINFO.Authors=x.XPathStringAll('css("a.author")')
		MANGAINFO.Genres=x.XPathStringAll('css("div.manga-tags > a")')
		x.XPathHREFAll('css("div.chapter-list > div.slot > a")', MANGAINFO.ChapterLinks, MANGAINFO.ChapterNames)
	end
	MANGAINFO.ChapterLinks.Reverse(); MANGAINFO.ChapterNames.Reverse()
		return no_error
	else
		return net_problem
	end
end

function getpagenumber()
	TASK.PageLinks.Clear()
	TASK.PageNumber = 0
	local imghost = 'https://img.ironmancdn.xyz'
	if HTTP.GET(MaybeFillHost(MODULE.RootURL, URL)) then
		local x=CreateTXQuery(HTTP.Document)
		local s=x.XPathString('//script[contains(., "img_list")]')
		s = GetBetween('var img_list =', ';', s)
		x.ParseHTML(s)
	if MODULE.Name == '11Toon' then
		local v = x.XPath('json(*)()')
		for i = 1, v.Count do
			local v1 = v.Get(i)
			local link = v1.ToString():gsub('https://11toon.com', imghost)
						TASK.PageLinks.Add(link..'?v=2')
		end
		else
		x.XPathStringAll('json(*)()', TASK.PageLinks)
	end
	else
		return false
	end
	return true
end

function getnameandlink()
	if MODULE.Name == '11Toon' then
		if HTTP.GET(MODULE.RootURL .. '/bbs/board.php?bo_table=toon_c&is=&sord=&type=upd&page=' .. (URL + 1)) then
		local x = CreateTXQuery(HTTP.Document)
		local v = x.XPath('//ul[@id="free-genre-list"]//li/a')
		for i = 1, v.Count do
			local v1 = v.Get(i)
			NAMES.Add(v1.GetAttribute('data-ga-event-label'));
			LINKS.Add(v1.GetAttribute('href'));
		end
		else
		return net_problem
		end
	else
		if HTTP.GET(MODULE.RootURL .. '/bbs/page.php?hid=manga_list&page=' .. (URL + 1)) then
		local x = CreateTXQuery(HTTP.Document)
		x.XPathHREFAll('css("div.manga-list-gallery div.manga-subject > a")', LINKS, NAMES)
		return no_error
		else
		return net_problem
		end
	end
end

function getdirectorypagenumber()
	if MODULE.Name == '11Toon' then
		if HTTP.GET(MODULE.RootURL .. '/bbs/board.php?bo_table=toon_c&is=&sord=&type=upd&page=1') then
			local x = CreateTXQuery(HTTP.Document)
			PAGENUMBER = tonumber(x.XPathString('//a[contains(@class, "pg_end")]/substring-after(@href, "&page=")')) or 1
			return true
		else
			return false
		end
	else
		if HTTP.GET(MODULE.RootURL .. '/bbs/page.php?hid=manga_list') then
			local x = CreateTXQuery(HTTP.Document)
			PAGENUMBER = tonumber(x.XPathString('//ul[@class="pagination"]/li[last()]/a/@href'):match('%((%d+)%)$')) or 1
			return true
		else
			return false
		end
	end
end

function Init()
	local cat = 'Raw'
	function AddWebsiteModule(id, site, url)
		local m=NewWebsiteModule()
		m.ID=id
		m.Category=cat
		m.Name=site
		m.RootURL=url
		m.OnGetInfo='getinfo'
		m.OnGetPageNumber='getpagenumber'
		m.OnGetNameAndLink='getnameandlink'
		m.OnGetDirectoryPageNumber = 'getdirectorypagenumber'
	end
	AddWebsiteModule('5285bd53ca6442b2b77d290fb8785e8f', 'MangaShow', 'https://mangashow.me')
	AddWebsiteModule('c7b742a4dc8a451fa95baec1aa183b39', '11Toon', 'https://www.11toon.com')
end

