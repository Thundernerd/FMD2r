function getinfo()
	MANGAINFO.URL=MaybeFillHost(MODULE.RootURL, URL)
	if HTTP.GET(MANGAINFO.URL) then
		local x=CreateTXQuery(HTTP.Document)
		MANGAINFO.Title=x.XPathString('css("div.titlesinfo_top > h2")')
		MANGAINFO.CoverLink=MaybeFillHost(MODULE.RootURL, x.XPathString('css("img.titlesinfo_coverimage")/@src'))
		MANGAINFO.Status=MangaInfoStatusIfPos(x.XPathString('//div[contains(span, "Status")]/span[@class="titleinfo_infovalue"]'))
		MANGAINFO.Summary=x.XPathStringAll('//div[contains(span, "Description")]/*[@class="titlesinfo_description"]/text()', '')

		local chapters=x.XPath('//table[@class="titlesinfo_chaptertable"]//tr')
		for i = 1, chapters.Count do
			local v1 = chapters.Get(i)
			local s = ''
			local vol = x.XPathString('td[1]', v1)
			local ch = x.XPathString('td[2]', v1)
			local title = x.XPathString('td[3]/a/text()', v1)
			if vol ~= '' then s = s .. string.format('Vol. %s', vol); end
			if s ~= '' then s = s .. ' '; end
			if ch ~= '' then s = s .. string.format('Ch. %s', ch); end
			if title ~= '' then
				if s ~= '' then s = s .. ' - '; end
				s = s .. title
			end

			if s ~= '' then
				MANGAINFO.ChapterLinks.Add(x.XPathString('td[3]/a/@href', v1))
				MANGAINFO.ChapterNames.Add(s)
			end
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
	HTTP.POST(MODULE.RootURL .. '/module/reader/ajax.php', 'readingtype=all')
	HTTP.Reset()
	if HTTP.GET(MaybeFillHost(MODULE.RootURL, URL)) then
		local x=CreateTXQuery(HTTP.Document)
		x.XPathStringAll('//div[@class="reader_mangaimagebox"]/img/@src', TASK.PageLinks)
		return true
	end
	return false
end

function getnameandlink()
	if HTTP.GET(MODULE.RootURL .. '/titles/page/' .. (URL + 1)) then
		local x = CreateTXQuery(HTTP.Document)
		x.XPathHREFAll('css("div.titlelist_name > a")', LINKS, NAMES)
		return no_error
	else
		return net_problem
	end
end

function getdirectorypagenumber()
	if HTTP.GET(MODULE.RootURL .. '/titles') then
		x = CreateTXQuery(HTTP.Document)
		PAGENUMBER = tonumber(x.XPathString('//div[@class="titles_pages"]/a[last()-1]')) or 1
		return no_error
	else
		return net_problem
	end
end

function Init()
	local m = NewWebsiteModule()
	m.Name = 'BoredomSociety'
	m.ID = '5177214a176e42d592c24fdacc1e81bd'
	m.RootURL = 'https://www.boredomsociety.xyz'
	m.Category = 'English-Scanlation'
	m.OnGetInfo='getinfo'
	m.OnGetPageNumber='getpagenumber'
	m.OnGetNameAndLink='getnameandlink'
	m.OnGetDirectoryPageNumber='getdirectorypagenumber'
end