function getinfo()
	MANGAINFO.URL=MaybeFillHost(MODULE.RootURL, URL)
	if HTTP.GET(MANGAINFO.URL) then
		local x=CreateTXQuery(HTTP.Document)
		if MANGAINFO.Title == '' then
			MANGAINFO.Title=x.XPathString('//h1[@class="page-title"]')
		end
		MANGAINFO.CoverLink=MaybeFillHost(MODULE.RootURL, x.XPathString('//img[@class="series"]/@src'))
		MANGAINFO.Authors=x.XPathStringAll('//li[@class="info" and contains(*, "Author")]/text()', '')
		MANGAINFO.Artists = MANGAINFO.Authors
		MANGAINFO.Genres=x.XPathStringAll('//li[@class="info" and contains(*, "Genre")]/text()', '')
		MANGAINFO.Status = MangaInfoStatusIfPos(x.XPathStringAll('//li[@class="info" and contains(*, "Status")]/text()', ''))
		MANGAINFO.Summary=x.XPathStringAll('//li[@class="summary"]/text()', '')
		x.XPathHREFAll('//div[@id="chapterlist"]/li/a',MANGAINFO.ChapterLinks,MANGAINFO.ChapterNames)
		MANGAINFO.ChapterLinks.Reverse(); MANGAINFO.ChapterNames.Reverse()
		HTTP.Reset()
		HTTP.Headers.Values['Referer'] = MANGAINFO.URL
		return no_error
	else
		return net_problem
	end
end

function getpagenumber()
	TASK.PageLinks.Clear()
	TASK.PageNumber = 0
	if HTTP.GET(MaybeFillHost(MODULE.RootURL, URL)) then
		local x=CreateTXQuery(HTTP.Document)
		TASK.PageNumber = x.XPathCount('(//select[@name="page"])[1]/option')
	else
		return false
	end
	return true
end

function getimageurl()
	local lurl=MaybeFillHost(MODULE.RootURL,URL)
	if WORKID~=0 then lurl=lurl..'/'..(WORKID+1) end
	if HTTP.GET(lurl) then
		local x=CreateTXQuery(HTTP.Document)
		local base = x.XPathString("//base/@href")
		TASK.PageLinks[WORKID]=MaybeFillHost(base, x.XPathString('//img[@class="picture"]/@src'))
		return true
	else
		return false
	end
end

function beforedownloadimage()
	HTTP.Headers.Values['Referer'] = MODULE.RootURL
	return true
end

function getnameandlink()
	if HTTP.GET(MODULE.RootURL .. '/comics-list') then
		local x = CreateTXQuery(HTTP.Document)
		x.XPathHREFAll('//div[@id="content-wrap"]//table//tr/td/div/span/a', LINKS, NAMES)
		return no_error
	else
		return net_problem
	end
end

function Init()
	local m = NewWebsiteModule()
	m.ID = 'ffe3cb9dc83f4f2da9628024aaedbef3'
	m.Name = 'ReadComicBooksOnline'
	m.RootURL = 'https://readcomicbooksonline.org'
	m.Category = 'English'
	m.LastUpdated='April 26, 2018'
	m.OnGetInfo='getinfo'
	m.OnGetPageNumber='getpagenumber'
	m.OnGetNameAndLink='getnameandlink'
	m.OnGetImageURL='getimageurl'
	m.OnBeforeDownloadImage = 'beforedownloadimage'
end
