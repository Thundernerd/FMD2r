function Init()
	function AddWebsiteModule(id, name, url)
		local m = NewWebsiteModule()
		m.ID                         = id
		m.Name                       = name
		m.RootURL                    = url
		m.Category                   = 'H-Sites'
		m.OnGetDirectoryPageNumber   = 'GetDirectoryPageNumber'
		m.OnGetNameAndLink           = 'GetNameAndLink'
		m.OnGetInfo                  = 'GetInfo'
		m.OnGetPageNumber            = 'GetPageNumber'
		m.SortedList                 = true
	end
	AddWebsiteModule('43c6880fe2804a198c6e9f748641498f', 'HentaiShark', 'https://www.hentaishark.com')
	AddWebsiteModule('39ab2e1385d84a93aa9d28d44a5a7099', 'ReadHentai', 'https://readhent.ai')
end

function GetDirectoryPageNumber()
	if HTTP.GET(MODULE.RootURL) then
		PAGENUMBER = tonumber(CreateTXQuery(HTTP.Document).XPathString('//ul[@class="pagination"]/li[last()-1]/a'))
		return no_error
	else
		return net_problem
	end
end

function GetNameAndLink()
	if HTTP.GET(MODULE.RootURL .. '/?page=' .. (URL + 1)) then
		local x = CreateTXQuery(HTTP.Document)
		local v; for v in x.XPath('//div[@class="item"]/a').Get() do
			LINKS.Add(v.GetAttribute('href'))
			NAMES.Add(x.XPathString('div[@class="caption"]', v))
		end
		return no_error
	else
		return net_problem
	end
end

function GetInfo()
	MANGAINFO.URL = MaybeFillHost(MODULE.RootURL, URL)
	if HTTP.GET(MANGAINFO.URL) then
		local x = CreateTXQuery(HTTP.Document)
		MANGAINFO.Title      = x.XPathString('//h2[contains(@class,"widget-title")]')
		MANGAINFO.CoverLink  = x.XPathString('//div[contains(@class,"show-manga")]//img/@src')
		MANGAINFO.Artists    = x.XPathStringAll('//dt[contains(.,"Artists")]/following-sibling::dd[1]/a')
		MANGAINFO.Genres     = x.XPathStringAll('//dd[@class="tag-links"]/a/text()[not(parent::span/@class)]')

		MANGAINFO.ChapterLinks.Add(URL)
		MANGAINFO.ChapterNames.Add(MANGAINFO.Title)
		return no_error
	else
		return net_problem
	end
end

function GetPageNumber()
	if HTTP.GET(MaybeFillHost(MODULE.RootURL, URL)) then
		local x = CreateTXQuery(HTTP.Document)
		CreateTXQuery(HTTP.Document).XPathStringAll('//img[contains(@class,"lazy")]/@src/replace(.,"thumb_","")', TASK.PageLinks)
		return true
	else
		return false
	end
end