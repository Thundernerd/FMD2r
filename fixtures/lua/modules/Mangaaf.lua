local dirurl = '/api/v1/explore/state/'
local dirstates = {'stopped', 'ongoing', 'completed'}

function GetNameAndLink()
	local s, p = dirstates[MODULE.CurrentDirectoryIndex + 1], (URL + 1)
	if HTTP.GET(MODULE.RootURL .. dirurl .. s .. '?page=' .. p) then
			local x = CreateTXQuery(HTTP.Document)
		if URL == '0' then
			s = x.XPathString('json(*).last_page')
			UPDATELIST.CurrentDirectoryPageNumber = x.XPathString('json(*).last_page') or 0
		end
		local v
		for v in x.XPath('json(*).data()').Get() do
			LINKS.Add(MODULE.RootURL .. '/m/' .. v.GetProperty('slug').ToString())
			NAMES.Add(v.GetProperty('name').ToString())
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
		local v = x.XPath('//div[@id="content"]/noscript/div[@class="container"]')
		MANGAINFO.CoverLink = x.XPathString('dl[dt="الكاتب"]/dd', v)
		MANGAINFO.Title     = x.XPathString('h2', v)
		MANGAINFO.Authors   = x.XPathString('dl[dt="الكاتب"]/dd', v)
		MANGAINFO.Artists   = x.XPathString('dl[dt="الراسم"]/dd', v)
		MANGAINFO.Genres    = x.XPathString('string-join(dl[dt="التصنيفات"]/dd/a,", ")')
		MANGAINFO.Summary   = x.XPathString('dl[dt="القصة"]/dd', v)
		MANGAINFO.Status    = MangaInfoStatusIfPos(x.XPathString('dl[dt="الحالة"]/dd', v),
			'مستمرة',
			'مكتملة')

		x.XPathHREFAll('div[@class="Chapters"]/ul/li/a', MANGAINFO.ChapterLinks, MANGAINFO.ChapterNames, v)
		MANGAINFO.ChapterLinks.Reverse(); MANGAINFO.ChapterNames.Reverse()
		return no_error
	else
		return net_problem
	end
end

function GetPageNumber()
	if HTTP.GET(MaybeFillHost(MODULE.RootURL, URL)) then
		CreateTXQuery(HTTP.Document).XPathStringAll('//noscript/div[@class="container"]/ul/li/img/@src', TASK.PageLinks)
		return true
	else
		return false
	end
end

function Init()
	local m = NewWebsiteModule()
	m.ID                         = 'cb80ee745b934469a5cc34e21e83e72b'
	m.Name                       = 'Mangaf'
	m.RootURL                    = 'https://mangaforall.com'
	m.Category                   = 'Arabic'
	m.TotalDirectory             = 3
	m.OnGetNameAndLink           = 'GetNameAndLink'
	m.OnGetInfo                  = 'GetInfo'
	m.OnGetPageNumber            = 'GetPageNumber'
end
