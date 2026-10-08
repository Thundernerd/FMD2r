function getinfo()
	MANGAINFO.URL=MaybeFillHost(MODULE.RootURL, URL)
	if HTTP.GET(MANGAINFO.URL) then
		local x=CreateTXQuery(HTTP.Document)
		if MANGAINFO.Title == '' then
			MANGAINFO.Title = x.XPathString('//h1[@class="page-title"]')
		end
		MANGAINFO.Summary = x.XPathString('//div[@class="archive-meta"]')
		x.XPathHREFAll('//div[@class="entry"]/table//tr/td[1]/a', MANGAINFO.ChapterLinks, MANGAINFO.ChapterNames)
		MANGAINFO.ChapterLinks.Reverse(); MANGAINFO.ChapterNames.Reverse()
		return no_error
	else
		return net_problem
	end
end

function getpagenumber()
	TASK.PageLinks.Clear()
	TASK.PageContainerLinks.Clear()
	TASK.PageNumber = 0
	if HTTP.GET(MaybeFillHost(MODULE.RootURL, URL)) then
		local x=CreateTXQuery(HTTP.Document)
		x.XPathStringAll('//select[@id="manga_pid"]/option/@value', TASK.PageContainerLinks)
		TASK.PageNumber = TASK.PageContainerLinks.Count
	else
		return false
	end
	return true
end

function getimageurl()
	if HTTP.GET(MaybeFillHost(MODULE.RootURL, URL):gsub('/+$', '') .. '/?pid=' .. TASK.PageContainerLinks[WORKID]) then
		local x = CreateTXQuery(HTTP.Document)
		TASK.PageLinks[WORKID] = x.XPathString('//div[@class="pic"]/a/img/@src')
		return true
	else
		return false
	end
end

function getnameandlink()
	if HTTP.GET(MODULE.RootURL .. '/%D9%82%D8%A7%D8%A6%D9%85%D8%A9-%D8%A7%D9%84%D9%85%D8%A7%D9%86%D8%AC%D8%A7/') then
		CreateTXQuery(HTTP.Document).XPathHREFAll('//ul[@class="MangaList"]/li//div[@class="SeriesName"]/a', LINKS, NAMES)
		return no_error
	else
		return net_problem
	end
end

function Init()
	local m = NewWebsiteModule()
	m.ID = '5f799e9581f94eb6a61079f2f3f76a94'
	m.Name = 'Mexat'
	m.RootURL = 'http://manga.mexat.com'
	m.Category = 'Arabic-Scanlation'
	m.LastUpdated='June 23, 2018'
	m.OnGetInfo='getinfo'
	m.OnGetPageNumber='getpagenumber'
	m.OnGetNameAndLink='getnameandlink'
	m.OnGetImageURL = 'getimageurl'
end
