function Init()
	local m = NewWebsiteModule()
	m.ID                         = 'e30f500d4eb0413e858dc071616228f1'
	m.Name                       = 'MangaHasu'
	m.RootURL                    = 'https://mangahasu.se'
	m.Category                   = 'English'
	m.OnGetNameAndLink           = 'GetNameAndLink'
	m.OnGetDirectoryPageNumber   = 'GetDirectoryPageNumber'
	m.OnGetInfo                  = 'GetInfo'
	m.OnGetPageNumber            = 'GetPageNumber'
	m.OnBeforeDownloadImage      = 'BeforeDownloadImage'
end

function GetNameAndLink()
	if HTTP.GET(MODULE.RootURL .. '/directory.html?page=' .. (URL + 1)) then
		local x = CreateTXQuery(HTTP.Document)
		x.XPathHREFAll('//ul[@class="list_manga"]/li//a[@class="name-manga"]', LINKS, NAMES)
		return no_error
	else
		return net_problem
	end
end

function GetDirectoryPageNumber()
	if HTTP.GET(MODULE.RootURL .. '/directory.html') then
		x = CreateTXQuery(HTTP.Document)
		PAGENUMBER = tonumber(x.XPathString('//div[@class="pagination-ct"]/a[last()]/substring-after(@href,"=")')) or 1
		return no_error
	else
		return net_problem
	end
end

function GetInfo()
	MANGAINFO.URL = MaybeFillHost(MODULE.RootURL, URL)
	if HTTP.GET(MANGAINFO.URL) then
		local x = CreateTXQuery(HTTP.Document)
		MANGAINFO.Title     = x.XPathString('//div[@class="info-title"]/h1')
		MANGAINFO.CoverLink = MaybeFillHost(MODULE.RootURL, x.XPathString('//div[contains(@class, "info-img")]/img/@src'))
		MANGAINFO.Authors   = x.XPathStringAll('//div[contains(b, "Author")]/span/a')
		MANGAINFO.Artists   = x.XPathStringAll('//div[contains(b, "Artist")]/span/a')
		MANGAINFO.Genres    = x.XPathStringAll('//div[contains(b, "Genre")]/span/a')
		MANGAINFO.Status    = MangaInfoStatusIfPos(x.XPathString('//div[contains(b, "Status")]/span/a'))
		MANGAINFO.Summary   = x.XPathString('//h3[contains(.,"Summary")]/following-sibling::*')

		x.XPathHREFAll('//div[contains(@class, "list-chapter")]/table//td[@class="name"]/a',MANGAINFO.ChapterLinks,MANGAINFO.ChapterNames)
		MANGAINFO.ChapterLinks.Reverse(); MANGAINFO.ChapterNames.Reverse()

		HTTP.Reset()
		HTTP.Headers.Values['Referer'] = MANGAINFO.URL
		return no_error
	else
		return net_problem
	end
end

function GetPageNumber()
	if HTTP.GET(MaybeFillHost(MODULE.RootURL, URL)) then
		CreateTXQuery(HTTP.Document).XPathStringAll('//div[@class="img"]/img/@src', TASK.PageLinks)
	else
		return false
	end
	return true
end

function BeforeDownloadImage()
	HTTP.Headers.Values['Referer'] = MODULE.RootURL
	return true
end
