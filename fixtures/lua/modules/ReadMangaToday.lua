function Init()
	local m = NewWebsiteModule()
	m.ID                       = '89eb675e8eb049c485a5a475f8f5c0c9'
	m.Name                     = 'ReadMangaToday'
	m.RootURL                  = 'https://www.readmng.com'
	m.Category                 = 'English'
	m.OnGetDirectoryPageNumber = 'GetDirectoryPageNumber'
	m.OnGetNameAndLink         = 'GetNameAndLink'
	m.OnGetInfo                = 'GetInfo'
	m.OnGetPageNumber          = 'GetPageNumber'
	m.SortedList               = true
end

function GetDirectoryPageNumber()
	if HTTP.GET(MODULE.RootURL .. '/manga-list/1?status=&type=&order=date.n-o') then
		PAGENUMBER = tonumber(CreateTXQuery(HTTP.Document).XPathString('//ul[@class="pagination"]/li[last()]/a/@href'):match('list/(.-)?')) or 1
		return no_error
	else
		return net_problem
	end
end

function GetNameAndLink()
	if HTTP.GET(MODULE.RootURL .. '/manga-list/' .. (URL + 1) .. '?status=&type=&order=date.n-o') then
		local x = CreateTXQuery(HTTP.Document)
		local v for v in x.XPath('//div[@class="mangaSliderCard"]/a').Get() do
			LINKS.Add(v.GetAttribute('href'))
			NAMES.Add(x.XPathString('div/h2', v))
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
		MANGAINFO.Title     = x.XPathString('//h1')
		MANGAINFO.CoverLink = x.XPathString('//div[@class="thumb"]//img/@src')
		MANGAINFO.Authors   = x.XPathStringAll('//div[(./b="Author")]//a')
		MANGAINFO.Artists   = x.XPathStringAll('//div[(./b="Artist")]//a')
		MANGAINFO.Genres    = x.XPathStringAll('//div[(./b="Genres")]//a')
		MANGAINFO.Status    = MangaInfoStatusIfPos(x.XPathString('//div[@class="imptdt"][1]/i'))
		MANGAINFO.Summary   = x.XPathString('//div[(./h2="Description")]/p')

		local v for v in x.XPath('//div[@class="checkBoxCard"]/a').Get() do
			MANGAINFO.ChapterLinks.Add(v.GetAttribute('href'))
			MANGAINFO.ChapterNames.Add(x.XPathString('text()', v))
		end
		MANGAINFO.ChapterLinks.Reverse(); MANGAINFO.ChapterNames.Reverse()
		return no_error
	else
		return net_problem
	end
end

function GetPageNumber()
	if HTTP.GET(MaybeFillHost(MODULE.RootURL, URL)) then
		local x = CreateTXQuery(HTTP.Document)
		x.ParseHTML(GetBetween('"images": ', '}],', x.XPathString('//script[contains(., "images")]')))
		local v for v in x.XPath('json(*)()').Get() do
			TASK.PageLinks.Add(v.ToString())
		end
		return true
	else
		return false
	end
end
