function Init()
	local m = NewWebsiteModule()
	m.ID                         = '01e9a8ebaa994307bef01780909e8cb7'
	m.Name                       = 'EarlyManga'
	m.RootURL                    = 'https://earlymangarelease.com'
	m.Category                   = 'Webcomics'
	m.OnGetDirectoryPageNumber   = 'GetDirectoryPageNumber'
	m.OnGetNameAndLink           = 'GetNameAndLink'
	m.OnGetInfo                  = 'GetInfo'
	m.OnGetPageNumber            = 'GetPageNumber'
end

function GetDirectoryPageNumber()
	if HTTP.GET(MODULE.RootURL .. '/manga?page=1') then
		PAGENUMBER = tonumber(CreateTXQuery(HTTP.Document).XPathString('//ul[@class="pagination"]/li[last()-1]')) or 1
		return no_error
	else
		return net_problem
	end
end

function GetNameAndLink()
	if HTTP.GET(MODULE.RootURL .. '/manga?page=' .. (URL + 1)) then
		CreateTXQuery(HTTP.Document).XPathHREFAll('//div[@class="titl-wrap"]/a', LINKS, NAMES)
		return no_error
	else
		return net_problem
	end
end

function GetInfo()
	MANGAINFO.URL = MaybeFillHost(MODULE.RootURL, URL)
	if HTTP.GET(MANGAINFO.URL) then
		local x = CreateTXQuery(HTTP.Document)
		MANGAINFO.Title      = x.XPathString('//span[@class="mx-1"]')
		MANGAINFO.CoverLink  = x.XPathString('//meta[@property="og:image"]/@content')
		MANGAINFO.Authors    = x.XPathStringAll('//div[contains(@class, "author-link")]/a')
		MANGAINFO.Artists    = x.XPathStringAll('//div[contains(@class, "artist-link")]/a')
		MANGAINFO.Genres     = x.XPathStringAll('//div[contains(@class, "generes-row")]//a')
		MANGAINFO.Status     = MangaInfoStatusIfPos(x.XPathString('//div[contains(@class, "pub_stutus")][1]'))
		local rem = x.XPathString('//div[contains(@class, "desc-row")]/div[3]/div')
		local summary = x.XPathString('//div[contains(@class, "desc-row")]/div[3]')
		summary = summary:match("^" .. rem .. "(.*)")
		MANGAINFO.Summary    = summary

		while true do
			local v for v in x.XPath('//div[@class="no-gutters"]//div[contains(@class, "chapter-row")]/div[2]/a').Get() do
				MANGAINFO.ChapterLinks.Add(v.GetAttribute('href'))
				MANGAINFO.ChapterNames.Add(x.XPathString('div', v))
			end
			p = x.XPathString('//ul[@class="pagination"]/li[contains(@class, "paging")]/a/@href')
			if (p ~= '') and HTTP.GET(MaybeFillHost(MODULE.RootURL, URL .. p)) then
				x.ParseHTML(HTTP.Document)
			else
				break
			end
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
		local v for v in x.XPath('//div[@class="chapter-images-container-up"]/img[not(@class)]/@src').Get() do
			TASK.PageLinks.Add(MaybeFillHost(MODULE.RootURL, v.ToString()))
		end
		return true
	else
		return false
	end
end
