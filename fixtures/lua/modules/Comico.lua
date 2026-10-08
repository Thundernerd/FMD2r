local apiUrl = 'http://www.comico.jp/api'
local getChaptersApiUrl = apiUrl .. '/getArticleList.nhn'
local dirurl = '/official'
local dirpages = {'mon', 'tue', 'wed', 'thu', 'fri', 'sat', 'sun', 'finish'}

function Init()
	local m = NewWebsiteModule()
	m.ID                         = 'ebb7036e4db04fb6a1d27bb4cdd10ee8'
	m.Name                       = 'Comico'
	m.RootURL                    = 'http://www.comico.jp'
	m.Category                   = 'Raw'
	m.OnGetNameAndLink           = 'GetNameAndLink'
	m.OnGetInfo                  = 'GetInfo'
	m.OnGetPageNumber            = 'GetPageNumber'
	m.OnBeforeDownloadImage      = 'BeforeDownloadImage'
	m.TotalDirectory             = #dirpages
end

-- Get Series Name
function GetNameAndLink()
	if HTTP.GET(MODULE.RootURL .. dirurl .. '/' .. dirpages[MODULE.CurrentDirectoryIndex + 1]) then
		local x, v = CreateTXQuery(HTTP.Document)
		for v in x.XPath('//ul[contains(@class, "resizeTitleList")]/li/a').Get() do
			LINKS.Add(v.GetAttribute('href'))
			NAMES.Add(x.XPathString('div/p[1]/text()', v))
		end
		return no_error
	else
		return net_problem
	end
end

function GetInfo()
	MANGAINFO.URL = MaybeFillHost(MODULE.RootURL, URL):gsub('/*$', '')
	if HTTP.GET(MANGAINFO.URL) then
		local x = CreateTXQuery(HTTP.Document)

		MANGAINFO.CoverLink = MaybeFillHost(MODULE.RootURL, x.XPathString('//meta[@property="og:image"]/@content'))
		MANGAINFO.Title     = Trim(SeparateLeft(x.XPathString('//meta[@property="og:title"]/@content'), '|'))
		MANGAINFO.Authors   = x.XPathStringAll('//*[contains(@class, "author")]/a')
		MANGAINFO.Genres    = x.XPathStringAll('//div[contains(@class, "meta")]/p[2]/a')
		MANGAINFO.Summary   = x.XPathString('//meta[@property="og:description"]/@content')
		MANGAINFO.Status    = MangaInfoStatusIfPos(x.XPathString('//div[contains(@class, "meta")]/p[1]'), '', '完結作品')

		local s = MANGAINFO.URL:match('(title[nN]o=%d+)')
		HTTP.Reset()
		if HTTP.POST(getChaptersApiUrl, s) then
			x.ParseHTML(HTTP.Document)
			for v in x.XPath('json(*).result.list()').Get() do
				MANGAINFO.ChapterLinks.Add(v.GetProperty('articleDetailUrl').ToString())
				s = v.GetProperty('freeFlg').ToString()
				if s == 'N' then
					s = s .. ' [unavailable]'
				else
					s = ''
				end
				MANGAINFO.ChapterNames.Add(v.GetProperty('subtitle').ToString() .. s)
			end
		end
		return no_error
	else
		return net_problem
	end
end

function GetPageNumber()
	if HTTP.GET(MaybeFillHost(MODULE.RootURL, URL):gsub('/*$', '')) then
		CreateTXQuery(HTTP.Document).XPathStringAll('//img[contains(@class, "comic-image")]/@src', TASK.PageLinks)
		return true
	else
		return false
	end
end

function BeforeDownloadImage()
	if TASK.CurrentDownloadChapterPtr < TASK.ChapterLinks.Count then
		HTTP.Headers.Values['Referer'] = ' ' .. MaybeFillHost(MODULE.RootURL, TASK.ChapterLinks[TASK.CurrentDownloadChapterPtr])
	end
	return true
end
