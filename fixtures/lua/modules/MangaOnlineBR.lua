function GetNameAndLink()
	if HTTP.GET(MODULE.RootURL..'/titulos/') then
			CreateTXQuery(HTTP.Document).XPathHREFAll('//div[@class="manga"]/p[1]/a', LINKS, NAMES)
		return no_error
	else
		return net_problem
	end
end

function GetInfo()
	MANGAINFO.URL = MaybeFillHost(MODULE.RootURL, URL)
	if HTTP.GET(MANGAINFO.URL) then
		local x = CreateTXQuery(HTTP.Document)

		MANGAINFO.CoverLink = MaybeFillHost(MODULE.RootURL, x.XPathString('//*[@id="page-mangas"]//*[@class="image"]/img/@src'))
		MANGAINFO.Title     = x.XPathString('//h1')
		MANGAINFO.Authors   = x.XPathString('//*[@class="texto-left"]/ul/li[starts-with(.,"História")]/substring-after(.,":")')
		MANGAINFO.Artists   = x.XPathString('//*[@class="texto-left"]/ul/li[starts-with(.,"Ilustração")]/substring-after(.,":")')
		MANGAINFO.Genres    = x.XPathString('string-join(//*[@class="generos"]/a,", ")')
		MANGAINFO.Summary   = x.XPathString('//p[.="Sinopse"]/following-sibling::p')
		MANGAINFO.Status    = MangaInfoStatusIfPos(x.XPathString('//*[@class="texto-left"]/ul/li[starts-with(.,"Status")]'), 'Em Publicação', '')

		local v, w, s
		for v in x.XPath('//*[@id="volumes-capitulos"]//*[@class="texto"]').Get() do
			s = x.XPathString('p[1]', v)
			for w in x.XPath('p[2]//a', v).Get() do
				MANGAINFO.ChapterLinks.Add(w.GetAttribute('href'))
				MANGAINFO.ChapterNames.Add(s .. ' Capitulo ' .. w.ToString())
			end
		end
		MANGAINFO.ChapterLinks.Reverse(); MANGAINFO.ChapterNames.Reverse()
		return no_error
	else
		return net_problem
	end
end

function GetPageNumber()
	local a, c = URL:match('/([^/]+)/capitulo/(%d+)')
	if a and c then
		local crypto = require 'fmd.crypto'
		if HTTP.GET(MODULE.RootURL .. '/capitulo.php?act=getImg&anime=' .. crypto.EncodeURLElement(a) .. '&capitulo=' .. c .. '&src=1&view=2') then
			local x = CreateTXQuery(HTTP.Document)
			local v, i = x.XPath('//*[@id="imgAvancadoVisualizacao"]/img')
			for i = 1, v.Count do
				TASK.PageLinks.Add(MaybeFillHost(MODULE.RootURL, v.Get(i).GetAttribute('src')))
			end
			return true
		else
			return false
		end
	end
end

function Init()
	local m = NewWebsiteModule()
	m.ID               = '6964c68b1d6846b2a2facae5fc56474e'
	m.Name             = 'MangaOnlineBR'
	m.RootURL          = 'http://mangaonline.com.br'
	m.Category         = 'Portuguese'
	m.OnGetNameAndLink = 'GetNameAndLink'
	m.OnGetInfo        = 'GetInfo'
	m.OnGetPageNumber  = 'GetPageNumber'
end
