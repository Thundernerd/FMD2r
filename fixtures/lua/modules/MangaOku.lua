function getinfo()
	MANGAINFO.URL=MaybeFillHost(MODULE.RootURL,URL)
	if HTTP.GET(MANGAINFO.URL) then
		x=CreateTXQuery(HTTP.Document)
		MANGAINFO.Title=x.XPathString('//select[@name="manga"]/option[@selected]')
		s=x.XPathString('//select[@name="manga"]/option[@selected]/@value')
		v=x.XPath('//select[@name="chapter"]/option')
		for i = 1, v.Count do
			v1=v.Get(i)
			MANGAINFO.ChapterNames.Add(v1.ToString())
			MANGAINFO.ChapterLinks.Add(MODULE.RootURL .. '/' .. s .. '/' .. v1.GetAttribute('value'))
		end
		MANGAINFO.ChapterLinks.Reverse(); MANGAINFO.ChapterNames.Reverse()
		return no_error
	else
		return net_problem
	end
end

function getpagenumber()
	TASK.PageLinks.Clear()
	TASK.PageNumber=0
	if HTTP.GET(MaybeFillHost(MODULE.RootURL,URL)) then
		local x=CreateTXQuery(HTTP.Document)
		TASK.PageNumber = x.XPath('//select[@name="page"]/option').Count
		return true
	else
		return false
	end
end

function getnameandlink()
	if HTTP.GET(MODULE.RootURL) then
		x=CreateTXQuery(HTTP.Document)
		v=x.XPath('//select[@name="manga"]/option[@value!="0"]')
		for i = 1, v.Count do
			v1=v.Get(i)
			NAMES.Add(v1.ToString())
			LINKS.Add(MODULE.RootURL .. '/' .. v1.GetAttribute('value'))
		end
		return no_error
	else
		return net_problem
	end
end

function getimageurl()
	local s = MaybeFillHost(MODULE.RootURL,URL) .. '/' .. tostring(WORKID+1)
	if HTTP.GET(s) then
		x=CreateTXQuery(HTTP.Document)
		TASK.PageLinks[WORKID]=MaybeFillHost(MODULE.RootURL,x.XPathString('//img[@id="manga_img"]/@src'))
		return true
	else
		return false
	end
end

function Init()
	function AddWebsiteModule(id, name, url)
		local m = NewWebsiteModule()
		m.ID = id
		m.Name = name
		m.RootURL = url
		m.Category = 'Turkish'
		m.LastUpdated = 'February 16, 2018'
		m.OnGetInfo = 'getinfo'
		m.OnGetPageNumber = 'getpagenumber'
		m.OnGetImageURL = 'getimageurl'
		m.OnGetNameAndLink = 'getnameandlink'
	end
	AddWebsiteModule('1642d82b6c0e4bb88bfaa62589dccb98', 'MangaOku', 'http://www.mangaoku.net')
	AddWebsiteModule('bdbd8eff725747d3a756b998c9f6dc9b', 'Turkcraft', 'http://turkcraft.com')
end