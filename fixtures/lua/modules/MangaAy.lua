function Init()
	local m = NewWebsiteModule()
	m.ID                  = '738b476a0fa5429bbc9de8e2ae5d00a6'
	m.Name                = 'MangaAy'
	m.RootURL             = 'https://manga-ay.com'
	m.Category            = 'Turkish'
	m.OnGetNameAndLink    = 'GetNameAndLink'
	m.OnGetInfo           = 'GetInfo'
	m.OnGetPageNumber     = 'GetPageNumber'
end

function GetNameAndLink()
	local dirurl = MODULE.RootURL .. '/seriler'
	if not HTTP.GET(dirurl) then return net_problem end
	local x = CreateTXQuery(HTTP.Document)
	local next_url
	while true do
		x.XPathHREFAll('//div[@class="table-responsive"]//td[2]/a', LINKS, NAMES)
		next_url = MaybeFillHost(MODULE.RootURL, x.XPathString('//ul[@class="pagination"]/li[last()-1]/a/@href'))
		if HTTP.Terminated then break end
		if next_url == '' then break end
		UPDATELIST.UpdateStatusText('Loading page ' .. (next_url:match('seriler/(%d+)/') or ''))
		if HTTP.GET(next_url) then
			x.ParseHTML(HTTP.Document)
		else
			break
		end
	end
	return no_error
end

function GetInfo()
	if HTTP.GET(MaybeFillHost(MODULE.RootURL, URL)) then
		local x = CreateTXQuery(HTTP.Document)
		MANGAINFO.Title     = x.XPathString('//h3[@class="panel-title"]')
		MANGAINFO.CoverLink = MaybeFillHost(MODULE.RootURL, x.XPathString('//div[@class="col-sm-3"]/img/@src'))
		MANGAINFO.Authors   = x.XPathStringAll('//th[contains(., "Yazar")]/following-sibling::td/a')
		MANGAINFO.Artists   = x.XPathStringAll('//th[contains(., "Çizer")]/following-sibling::td/a')
		MANGAINFO.Genres    = x.XPathStringAll('//th[contains(., "Türler")]/following-sibling::td//a')
		MANGAINFO.Status    = MangaInfoStatusIfPos(x.XPathString('//th[contains(., "Durum")]/following-sibling::td'), 'Devam Ediyor', 'Tamamlandı')
		MANGAINFO.Summary   = x.XPathString('//th[contains(., "Açıklama")]/following-sibling::td')

		x.XPathHREFAll('//div[@class="table-responsive"]//td[2]/a', MANGAINFO.ChapterLinks, MANGAINFO.ChapterNames)
		MANGAINFO.ChapterLinks.Reverse(); MANGAINFO.ChapterNames.Reverse()
		return no_error
	else
		return net_problem
	end
end

function GetPageNumber()
	if HTTP.GET(MaybeFillHost(MODULE.RootURL, URL)) then
		local body    = HTTP.Document.ToString()
		local dataurl = body:match("var dataurl = '(.-)'")
		local server  = body:match("var server = '(.-)'")
		local images  = body:match("var page_array = %[([^%]]+)")
		local i for i in images:gmatch("'([^',]+)") do
			TASK.PageLinks.Add(MaybeFillHost(MODULE.RootURL, server .. dataurl .. '/' .. i))
		end
		return true
	else
		return false
	end
end