function getinfo()
	MANGAINFO.URL=MaybeFillHost(MODULE.RootURL, URL)
	if HTTP.GET(MANGAINFO.URL) then
		local x=CreateTXQuery(HTTP.Document)
		if MANGAINFO.Title == '' then
			MANGAINFO.Title = x.XPathString('//div[@class="col-md-12"]/h3')
		end
		MANGAINFO.CoverLink = MaybeFillHost(MODULE.RootURL, x.XPathString('//div[@class="col-md-12"]/div/img[contains(@class, "img")]/@src'))
		MANGAINFO.Status = MangaInfoStatusIfPos(x.XPathString('//table[contains(@class, "info_list")]//tr[contains(td, "Status")]/td[2]'))
		MANGAINFO.Authors=x.XPathString('//table[contains(@class, "info_list")]//tr[contains(td, "Author")]/td[2]')
		MANGAINFO.Artists=x.XPathString('//table[contains(@class, "info_list")]//tr[contains(td, "Artist")]/td[2]')
		MANGAINFO.Genres=x.XPathStringAll('//table[contains(@class, "info_list")]//tr[contains(td, "Genres")]/td[2]/a')
		MANGAINFO.Summary=x.XPathString('//div[contains(@class, "summary")]/p')
		x.XPathHREFAll('//table[contains(@class,"chapter-list")]//tr/td[1]/a', MANGAINFO.ChapterLinks, MANGAINFO.ChapterNames)
		MANGAINFO.ChapterLinks.Reverse(); MANGAINFO.ChapterNames.Reverse()
		return no_error
	else
		return net_problem
	end
end

function getpagenumber()
	TASK.PageLinks.Clear()
	if HTTP.GET(MaybeFillHost(MODULE.RootURL, '/manga' .. URL)) then
		local x=CreateTXQuery(HTTP.Document)
		x.XPathStringAll('//div[@class="page_container"]/img[contains(@class, "manga_page")]/@src', TASK.PageLinks)
	else
		return false
	end
	return true
end

function getnameandlink()
	local dirurl = string.format('/search?search-words=&status=%d-page-%s', MODULE.CurrentDirectoryIndex, (URL + 1))
	if HTTP.GET(MODULE.RootURL..dirurl) then
		local x = CreateTXQuery(HTTP.Document)
		if UPDATELIST.CurrentDirectoryPageNumber <= 1 then
			UPDATELIST.CurrentDirectoryPageNumber = tonumber(x.XPathString('//ul[contains(@class,"pagination")]/li[last()-1]/a/@href'):match('page%-(%d+)')) or 1
		end
		x.XPathHREFAll('//div[@id="content_item"]//div[@class="manga_title"]/a', LINKS, NAMES)
		return no_error
	else
		return net_problem
	end
end

function Init()
	local m=NewWebsiteModule()
	m.ID = 'ca32c2c6c44c4b86be32ee47d40b04a3'
	m.Category='English'
	m.Name='MangaRoom'
	m.RootURL='http://manga-room.com'
	m.LastUpdated='April 8, 2018'
	m.OnGetInfo='getinfo'
	m.OnGetPageNumber='getpagenumber'
	m.OnGetNameAndLink='getnameandlink'
	m.TotalDirectory=2
end
