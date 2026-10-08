function GetInfo()
	MANGAINFO.URL=MaybeFillHost(MODULE.RootURL,URL)
	if HTTP.GET(MANGAINFO.URL) then
		x=CreateTXQuery(HTTP.Document)
		MANGAINFO.Title=x.XPathString('//h1')
		x.XPathHREFAll('//table//td/a',MANGAINFO.ChapterLinks,MANGAINFO.ChapterNames)
		MANGAINFO.ChapterLinks.Reverse(); MANGAINFO.ChapterNames.Reverse()
		return no_error
	else
		return net_problem
	end
end

function GetPageNumber()
	if HTTP.GET(MaybeFillHost(MODULE.RootURL,URL)) then
		x=CreateTXQuery(HTTP.Document)
		TASK.PageNumber=tonumber(x.XPathString('//div[contains(@class,"btn-reader-page")]/ul[@class="dropdown-menu"]/li[last()]/substring-before(substring-after(.,"("),")")'))
		return true
	else
		return false
	end
end

function GetImageURL()
	if HTTP.GET(MaybeFillHost(MODULE.RootURL,URL):gsub('/1$','')..'/'..tostring(WORKID+1)) then
		x=CreateTXQuery(HTTP.Document)
		TASK.PageLinks[WORKID]=x.XPathString('//img[@id="manga-page"]/@src')
		return true
	else
		return false
	end
end

function GetNameAndLink()
	if HTTP.GET(MODULE.RootURL..'/manga') then
		x=CreateTXQuery(HTTP.Document)
		x.XPathHREFAll('//table//tr/td[1]//a', LINKS, NAMES)
		return no_error
	else
		return net_problem
	end
end

function Init()
	local m = NewWebsiteModule()
	m.ID = 'cff79db51a6140bbac7e06eaa2ed9524'
	m.Category='English-Scanlation'
	m.Name='MangaStream'
	m.RootURL='https://readms.net'
	m.LastUpdated='February 8, 2018'
	m.OnGetInfo='GetInfo'
	m.OnGetPageNumber='GetPageNumber'
	m.OnGetImageURL='GetImageURL'
	m.OnGetNameAndLink='GetNameAndLink'
end
