function getinfo()
	MANGAINFO.URL=MaybeFillHost(MODULE.RootURL, URL)
	if HTTP.GET(MANGAINFO.URL) then
		local x=CreateTXQuery(HTTP.Document)
		MANGAINFO.Title=x.XPathString('//div[@class="manga-info"]//h1')
		MANGAINFO.CoverLink=MaybeFillHost(MODULE.RootURL, x.XPathString('//div[@class="manga-info"]/div[contains(@class, "manga-detail")]//img/@src'))
		MANGAINFO.Authors=x.XPathString('//div[@class="manga-info"]/div[contains(@class, "manga-detail")]//ul/li[contains(span, "Tác giả")]/span[2]')
		MANGAINFO.Artists=x.XPathString('//div[@class="manga-info"]/div[contains(@class, "manga-detail")]//ul/li[contains(span, "Họa sĩ")]/span[2]')
		MANGAINFO.Genres=x.XPathStringAll('//div[@class="manga-info"]/div[contains(@class, "manga-detail")]//ul/li[contains(span, "Thể loại")]/a')
		MANGAINFO.Status = MangaInfoStatusIfPos(x.XPathString('//div[@class="manga-info"]/div[contains(@class, "manga-detail")]//ul/li[contains(span, "Trạng thái")]'), 'Đang thực hiện', 'Đã hoàn thành')
		MANGAINFO.Summary=x.XPathString('//div[contains(@class, "manga-summary")]')
		x.XPathHREFAll('//div[contains(@class,"manga-chapter")]//ul/li/a', MANGAINFO.ChapterLinks, MANGAINFO.ChapterNames)
		MANGAINFO.ChapterLinks.Reverse(); MANGAINFO.ChapterNames.Reverse()
		return no_error
	else
		return net_problem
	end
end

function getpagenumber()
	TASK.PageLinks.Clear()
	TASK.PageNumber=0
	if HTTP.GET(MaybeFillHost(MODULE.RootURL, URL)) then
		local x=CreateTXQuery(HTTP.Document)
		x.XPathStringAll('//div[contains(@class, "manga-chapter-image")]/textarea/img/@src', TASK.PageLinks)
	else
		return false
	end
	return true
end

local dirurl = '/noi-bat/'
local perpage = 30

function getnameandlink()
	if tonumber(URL) < 0 then return no_error end
	if HTTP.GET(MODULE.RootURL .. dirurl .. tostring(tonumber(URL) * perpage)) then
		local x = CreateTXQuery(HTTP.Document)
		x.XPathHREFAll('//div[contains(@class, "manga-list")]//div[contains(@class,"tit")]/a', LINKS, NAMES)
		local s = x.XPathString('(//ul[@class="pagination"])[1]/li[last()-1]/a/@href')
		s = tonumber(s:match('(%d+)/?$'))
		if s ~= nil then
			s = s / perpage
			UPDATELIST.CurrentDirectoryPageNumber = s
		end
		return no_error
	else
		return net_problem
	end
end

function Init()
	local m = NewWebsiteModule()
	m.ID = '25eaced673ab4f84ba6a29900fd86d60'
	m.Name = 'ComicVn'
	m.RootURL = 'https://beeng.net'
	m.Category = 'Vietnamese'
	m.OnGetInfo='getinfo'
	m.OnGetPageNumber='getpagenumber'
	m.OnGetNameAndLink='getnameandlink'
end