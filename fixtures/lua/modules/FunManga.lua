local ALPHA_LIST = '#abcdefghijklmnopqrstuvwxyz'

function getinfo()
	MANGAINFO.URL=MaybeFillHost(MODULE.RootURL, URL)
	if HTTP.GET(MANGAINFO.URL) then
		local x=CreateTXQuery(HTTP.Document)
		MANGAINFO.Title=x.XPathString('//div[@class="content"]//h5')
		MANGAINFO.CoverLink=MaybeFillHost(MODULE.RootURL, x.XPathString('//div[@class="content"]//img/@src'))
		MANGAINFO.Authors=x.XPathStringAll('//dl[@class="dl-horizontal"]/dt[starts-with(.,"Author")]/following-sibling::dd[1]/a')
		MANGAINFO.Artists=x.XPathStringAll('//dl[@class="dl-horizontal"]/dt[starts-with(.,"Artist")]/following-sibling::dd[1]/a')
		MANGAINFO.Genres=x.XPathStringAll('//dl[@class="dl-horizontal"]/dt[starts-with(.,"Categories")]/following-sibling::dd[1]/a')
		MANGAINFO.Status = MangaInfoStatusIfPos(x.XPathString('//dl[@class="dl-horizontal"]/dt[starts-with(.,"Status")]/following-sibling::dd[1]'))
		MANGAINFO.Summary=x.XPathString('//div[@class="content"]/div/div[contains(@class,"note")]')
		local v=x.XPath('//ul[@class="chapter-list"]/li/a')
		for i=1,v.Count do
			local v1=v.Get(i)
			MANGAINFO.ChapterLinks.Add(v1.GetAttribute('href'))
			MANGAINFO.ChapterNames.Add(x.XPathString('span[1]', v1))
		end
		MANGAINFO.ChapterLinks.Reverse(); MANGAINFO.ChapterNames.Reverse()
		return no_error
	else
		return net_problem
	end
end

function getpagenumber()
	TASK.PageLinks.Clear()
	TASK.PageNumber=0
	local s = MaybeFillHost(MODULE.RootURL, URL)
	if string.find(s, '/all-pages', 1, true) == nil then s = s .. '/all-pages' end
	if HTTP.GET(s) then
		local x=CreateTXQuery(HTTP.Document)
		x.XPathStringAll('//div[contains(@class,"content-inner")]//img/@src', TASK.PageLinks)
	else
		return false
	end
	return true
end

function getnameandlink()
	local s = ''
	if MODULE.CurrentDirectoryIndex ~= 0 then
		s = '/'..ALPHA_LIST:sub(MODULE.CurrentDirectoryIndex+1,MODULE.CurrentDirectoryIndex+1)
	end
	local dirurl = '/manga-list'
	if MODULE.Name == 'MangaDoom' then dirurl = '/manga-directory' end
	if HTTP.GET(MODULE.RootURL .. dirurl .. s) then
		local x = CreateTXQuery(HTTP.Document)
		x.XPathHREFAll('//div[@class="content"]/div/div[@class="row"]//li/a', LINKS, NAMES)
		return no_error
	else
		return net_problem
	end
end

function Init()
	function AddWebsiteModule(id, name, URL)
		local m = NewWebsiteModule()
		m.ID               = id
		m.Name             = name
		m.RootURL          = URL
		m.Category         = 'English'
		m.LastUpdated      = 'March 1, 2018'
		m.OnGetInfo        = 'getinfo'
		m.OnGetPageNumber  = 'getpagenumber'
		m.OnGetNameAndLink = 'getnameandlink'
		m.TotalDirectory   = ALPHA_LIST:len()
	end
	AddWebsiteModule('aec40749e25a4fe9963d12bd1ad75b3d', 'FunManga', 'http://www.funmanga.com')
	AddWebsiteModule('2f6e385022144072bfd8407a004a2891', 'MangaDoom', 'http://www.mngdoom.com')
end
