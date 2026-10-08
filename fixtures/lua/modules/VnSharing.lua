function GetInfo()
	MANGAINFO.URL=MaybeFillHost(MODULE.RootURL,URL)
	if HTTP.GET(MANGAINFO.URL) then
		x=CreateTXQuery(HTTP.Document)
		MANGAINFO.Title=x.XPathString('//p[@class="title"]')
		MANGAINFO.CoverLink=MaybeFillHost(MODULE.RootURL, GetBetween("URL('", "')", x.XPathString('//div[contains(@class, "info_ava")]/@style')))
		MANGAINFO.Status = MangaInfoStatusIfPos(x.XPathString('//div[@class="info"]/span[contains(., "Status")]/a'), 'Đang tiến hành', 'Đang phát hành')
		MANGAINFO.Authors = x.XPathStringAll('//div[@id="manga_detail"]/ul/li[contains(b, "Tác giả")]/text()', '')
		MANGAINFO.Artists = x.XPathStringAll('//div[@class="info"]/span[contains(., "Artist")]/a')
		MANGAINFO.Genres = x.XPathStringAll('//div[@id="manga_detail"]/ul/li[contains(b, "Thể loại")]/a')
		MANGAINFO.Summary = x.XPathString('//p[@class="desc"]')
		x.XPathHREFAll('//ul[@id="manga-info-list"]/li[not(contains(@style, "none"))]/a[1]', MANGAINFO.ChapterLinks,MANGAINFO.ChapterNames)
		MANGAINFO.ChapterLinks.Reverse(); MANGAINFO.ChapterNames.Reverse()
		return no_error
	else
		return net_problem
	end
end

function getdirectorypagenumber()
	-- TODO: dynamic page count
	if HTTP.GET(MODULE.RootURL .. '/index/KhamPha/newest') then
		x = CreateTXQuery(HTTP.Document)
		PAGENUMBER = tonumber(x.XPathString('(//div[@class="pagination_wrap"]/a)[last()-1]')) or 1
		return no_error
	else
		return net_problem
	end
end

function GetPageNumber()
	if HTTP.GET(MaybeFillHost(MODULE.RootURL,URL)) then
		x=CreateTXQuery(HTTP.Document)
		x.XPathStringAll('//img[@id="manga_page"]/@src', TASK.PageLinks)
		return true
	else
		return false
	end
end

function GetNameAndLink()
	if HTTP.GET(MODULE.RootURL .. '/index/KhamPha/newest/' .. (URL + 1)) then
		local x = CreateTXQuery(HTTP.Document)
		local v = x.XPath('//ul[@id="browse_result_wrap"]/li[@class="browse_result_item"]/a[@class="title"]', LINKS, NAMES)
		for i = 1, v.Count do
			local v1 = v.Get(i)
			LINKS.Add(v1.GetAttribute('href'))
			NAMES.Add(v1.ToString())
		end
		if v.Count > 0 then
			UPDATELIST.CurrentDirectoryPageNumber = tonumber(x.XPathString('(//div[@class="pagination_wrap"]/a)[last()-1]')) or 1
		end
		return no_error
	else
		return net_problem
	end
end

function Init()
	local m = NewWebsiteModule()
	m.ID               = '1b67bcec1f5c483e9a6c05a96e7dc779'
	m.Category         = 'Vietnamese'
	m.Name             = 'VnSharing'
	m.RootURL          = 'http://truyen.vnsharing.site'
	m.SortedList       = true
	m.LastUpdated      = 'February 16, 2018'
	m.OnGetInfo        = 'GetInfo'
	m.OnGetPageNumber  = 'GetPageNumber'
	m.OnGetNameAndLink = 'GetNameAndLink'
end