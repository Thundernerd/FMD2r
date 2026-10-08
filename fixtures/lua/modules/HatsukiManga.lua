function Init()
	local m = NewWebsiteModule()
	m.ID                  = '13c4aabe55424dd2b46384102d1fae1e'
	m.Name                = 'HatsukiManga'
	m.RootURL             = 'https://hatsukimanga.com'
	m.Category            = 'Spanish'
	m.OnGetNameAndLink    = 'GetNameAndLink'
	m.OnGetInfo           = 'GetInfo'
	m.OnGetPageNumber     = 'GetPageNumber'
end

function GetNameAndLink()
	if HTTP.GET(MODULE.RootURL .. '/biblioteca.php') then
		CreateTXQuery(HTTP.Document).XPathHREFAll('//a[contains(@class, "miniatura")]', LINKS, NAMES)
		return no_error
	else
		return net_problem
	end
end

function GetInfo()
	MANGAINFO.URL = MaybeFillHost(MODULE.RootURL, URL)
	if HTTP.GET(MANGAINFO.URL) then
		local x = CreateTXQuery(HTTP.Document)
		MANGAINFO.Title      = x.XPathString('//h2[@class="titulo-pag-obra"]')
		MANGAINFO.CoverLink  = MaybeFillHost(MODULE.RootURL, x.XPathString('//img[@class="portada-obra-pag-obra"]/@src'))
		MANGAINFO.Genres     = x.XPathStringAll('//div[@class="generos-pag-obra"]/h4')
		MANGAINFO.Summary    = x.XPathString('//div[@class="desc-obra"]/p')

		HTTP.Reset()
		HTTP.Headers.Values['Cache-Control'] = ' no-cache'
		HTTP.MimeType = 'application/x-www-form-urlencoded'
		local q = 'idObra=' .. URL:match('id=(%d+)')
		if HTTP.POST(MODULE.RootURL .. '/src/get_all_caps.php', q) then
			local x = CreateTXQuery(HTTP.Document)
			local v for v in x.XPath('//div[@class="cuadro-obra"]').Get() do
				MANGAINFO.ChapterLinks.Add(x.XPathString('ul/li/div[2]/a/@href', v))
				MANGAINFO.ChapterNames.Add(x.XPathString('div/p', v) .. ' [' .. x.XPathString('ul/li/div[1]/a', v) .. ']')
			end
		end
		return no_error
	else
		return net_problem
	end
end

function GetPageNumber()
	if HTTP.GET(MaybeFillHost(MODULE.RootURL, '/biblioteca/' .. URL)) then
		local x = CreateTXQuery(HTTP.Document)
		local v for v in x.XPath('//li[@class="li-pagina"]/img/@src').Get() do
			TASK.PageLinks.Add(MaybeFillHost(MODULE.RootURL, v.ToString()))
		end
		return true
	else
		return false
	end
end
