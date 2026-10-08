function Init()
	local m = NewWebsiteModule()
	m.ID                         = '4f40515fb43640ddb08eb61278fc97a5'
	m.Name                       = 'KissManga'
	m.RootURL                    = 'https://kissmanga.org'
	m.Category                   = 'English'
	m.OnGetDirectoryPageNumber   = 'GetDirectoryPageNumber'
	m.OnGetNameAndLink           = 'GetNameAndLink'
	m.OnGetInfo                  = 'GetInfo'
	m.OnGetPageNumber            = 'GetPageNumber'
	m.SortedList                 = true
end

function GetDirectoryPageNumber()
	local url = MODULE.RootURL .. '/manga_list'
	if HTTP.GET(url) then
		PAGENUMBER = tonumber(CreateTXQuery(HTTP.Document).XPathString('//ul[@class="pager"]/li[last()]/a'):match('%((.-)%)')) or 1
		return no_error
	else
		return net_problem
	end
end

function GetNameAndLink()
	local url = MODULE.RootURL .. '/manga_list'
	if URL ~= '0' then
		url = url .. '?page=' .. (URL + 1)
	end
	if HTTP.GET(url) then
		CreateTXQuery(HTTP.Document).XPathHREFAll('//a[@class="item_movies_link"]', LINKS, NAMES)
		return no_error
	else
		return net_problem
	end
end

function GetInfo()
	MANGAINFO.URL=MaybeFillHost(MODULE.RootURL,URL)
	if HTTP.GET(MANGAINFO.URL) then
		local x = CreateTXQuery(HTTP.Document)
		MANGAINFO.Title     = x.XPathString('//strong[@class="bigChar"]')
		MANGAINFO.CoverLink = MaybeFillHost(MODULE.RootURL, x.XPathString('//div[@id="rightside"]//img/@src'))
		MANGAINFO.Authors   = x.XPathStringAll('//*[@class="info"][2]//a[@class="dotUnder"]')
		MANGAINFO.Artists   = x.XPathStringAll('//div[@class="barContent"]//span[starts-with(., "Artist")]/parent::*/a')
		MANGAINFO.Summary   = x.XPathString('//div[@class="summary"]//p')
		MANGAINFO.Genres    = x.XPathStringAll('//*[@class="info"][3]//a[@class="dotUnder"]'):gsub("%s+Manga", "")
		MANGAINFO.Status    = MangaInfoStatusIfPos(x.XPathString('//*[@class="item_static"][1]'), 'Ongoing', 'Completed')
		x.XPathHREFAll('//div[@class="listing listing8515 full"]//div//div//h3//a', MANGAINFO.ChapterLinks, MANGAINFO.ChapterNames)
		MANGAINFO.ChapterLinks.Reverse(); MANGAINFO.ChapterNames.Reverse()
		return no_error
	else
		return net_problem
	end
end

function GetPageNumber()
	if HTTP.GET(MaybeFillHost(MODULE.RootURL, URL)) then
		local x = CreateTXQuery(HTTP.Document)
		x.XPathStringAll('//div[@id="centerDivVideo"]/img/@src', TASK.PageLinks)
		return true
	else
		return false
	end
end
