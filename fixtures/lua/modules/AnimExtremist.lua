function getinfo()
	MANGAINFO.URL=MaybeFillHost(MODULE.RootURL, URL)
	if HTTP.GET(MANGAINFO.URL) then
		x=CreateTXQuery(HTTP.Document)
		MANGAINFO.Title=x.XPathString('//div[@id="LayerContenido"]/div[@id][1]')
		MANGAINFO.CoverLink=MaybeFillHost(MODULE.RootURL,x.XPathString('//div[@id="LayerContenido"]/div[@id][2]//tr[1]/td[2]//img/@src'))
		MANGAINFO.Genres=x.XPathString('string-join(//div[@id="LayerContenido"]/div[@id][2]//tr[5]//a,", ")')
		MANGAINFO.Status=MangaInfoStatusIfPos(x.XPathString('//div[@id="LayerContenido"]/div[@id][2]//tr[6]/td[1]'),'En proceso','Completo')
		MANGAINFO.Summary=x.XPathString('//div[@id="LayerContenido"]/div[@id][2]//tr[1]/td[1]')
		x.XPathStringAll('//div[@id="tomo"]//a/@href',MANGAINFO.ChapterLinks)
		x.XPathStringAll('//div[@id="tomo"]//a/string-join((../b,.)," ")',MANGAINFO.ChapterNames)
		MANGAINFO.ChapterLinks.Reverse(); MANGAINFO.ChapterNames.Reverse()
		return no_error
	else
		return net_problem
	end
end

function getpagenumber()
	if HTTP.GET(MaybeFillHost(MODULE.RootURL,URL):gsub('%.html?$','-1%1')) then
		TASK.PageNumber=CreateTXQuery(HTTP.Document).XPathCount('//select[@id="nav-jump"]/option')
		return true
	else
		return false
	end
	return true
end

function getimageurl()
	if HTTP.GET(MaybeFillHost(MODULE.RootURL,URL):gsub('%.html?$','-'..(WORKID+1)..'%1')) then
		TASK.PageLinks[WORKID]=CreateTXQuery(HTTP.Document).XPathString('//img[@id="photo"]/@src')
		return true
	end
	return false
end

function getnameandlink()
	if HTTP.GET(MODULE.RootURL..'/mangas.htm?ord=todos') then
		CreateTXQuery(HTTP.Document).XPathHREFAll('//*[@id="manga"]/div/a',LINKS,NAMES)
		return no_error
	else
		return net_problem
	end
end

function Init()
	local m = NewWebsiteModule()
	m.ID               = 'f79e98b0bbfd419fb96dfc7bf5055d3d'
	m.Category         = 'Spanish'
	m.Name             = 'AnimExtremist'
	m.RootURL          = 'http://www.animextremist.com'
	m.LastUpdated      = 'February 18, 2018'
	m.OnGetInfo        = 'getinfo'
	m.OnGetPageNumber  = 'getpagenumber'
	m.OnGetImageURL    = 'getimageurl'
	m.OnGetNameAndLink = 'getnameandlink'
end
