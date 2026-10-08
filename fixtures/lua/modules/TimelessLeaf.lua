function getinfo()
	MANGAINFO.URL=MaybeFillHost(MODULE.RootURL, URL)
	if HTTP.GET(MANGAINFO.URL) then
		local x=CreateTXQuery(HTTP.Document)
		MANGAINFO.Title=x.XPathString('//h1[@class="entry-title"]')
		MANGAINFO.CoverLink=MaybeFillHost(MODULE.RootURL, x.XPathString('//article//img/@src'))
		MANGAINFO.Summary=x.XPathStringAll('//div[@class="entry-content page-content"]/p[not(contains(., "Chapter"))]/text()', '')
		x.XPathHREFAll('//div[@class="entry-content page-content"]/p/a',MANGAINFO.ChapterLinks,MANGAINFO.ChapterNames)
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
		x.XPathStringAll('//figure[@class="gallery-item"]//img/@src', TASK.PageLinks)
	else
		return false
	end
	return true
end

function getnameandlink()
	if HTTP.GET(MODULE.RootURL .. '/comic-list') then
		local x = CreateTXQuery(HTTP.Document)
		x.XPathHREFAll('//a[.="Manga"]/following-sibling::ul//li/a[not(contains(@href,"more"))]', LINKS, NAMES)
		return no_error
	else
		return net_problem
	end
end

function Init()
	local m = NewWebsiteModule()
	m.ID = 'f202f923b0a94b5f8f1da98c02eaec3e'
	m.Name = 'TimelessLeaf'
	m.RootURL = 'https://timelessleaf.com'
	m.Category = 'English-Scanlation'
	m.OnGetInfo='getinfo'
	m.OnGetPageNumber='getpagenumber'
	m.OnGetNameAndLink='getnameandlink'
end