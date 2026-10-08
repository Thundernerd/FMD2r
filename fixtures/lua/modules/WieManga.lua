local ALPHA_LIST_UP = '#ABCDEFGHIJKLMNOPQRSTUVWXYZ'

function GetInfo()
	MANGAINFO.URL=MaybeFillHost(MODULE.RootURL,URL)
	if HTTP.GET(MANGAINFO.URL) then
		x=CreateTXQuery(HTTP.Document)
		MANGAINFO.CoverLink=MaybeFillHost(MODULE.RootURL, x.XPathString('//div[@class="bookfrontpage"]/a/img/@src'))
		if MODULE.Name == 'WieManga' then
			MANGAINFO.Title=x.XPathString('//div[@class="bookmessagebox"]/h1/substring-before(., " Manga")')
			MANGAINFO.Summary = x.XPathString('//h4[text()="Beschreibung"]/following-sibling::text()')
			MANGAINFO.Artists = x.XPathStringAll('//div[@class="bookmessgae"]//dd[contains(span/text(), "Zeichner")]/a')
			MANGAINFO.Authors = x.XPathStringAll('//div[@class="bookmessgae"]//dd[contains(span/text(), "Autor")]/a')
			MANGAINFO.Genres = x.XPathStringAll('//div[@class="bookmessgae"]//dd[contains(span/text(), "Genre")]/a')
			MANGAINFO.Status = MangaInfoStatusIfPos(x.XPathString('//div[@class="bookmessgae"]//dd[contains(span/text(), "Status")]/a'), 'ongoing', 'finished')
		elseif MODULE.Name == 'MangaRussia' then
			MANGAINFO.Title=x.XPathString('//div[@class="bookmessagebox"]/h1/substring-after(., "Манга ")')
			MANGAINFO.Summary = x.XPathString('//h4[text()="Описание"]/following-sibling::text()')
			MANGAINFO.Authors = x.XPathStringAll('//div[@class="bookmessgae"]//dd[contains(span/text(), "Автор")]/a')
			MANGAINFO.Genres = x.XPathStringAll('//div[@class="bookmessgae"]//dd[contains(span/text(), "Жанры")]/a')
			MANGAINFO.Status = MangaInfoStatusIfPos(x.XPathString('//div[@class="bookmessgae"]//dd[contains(span/text(), "Перевод")]/a'), 'ongoing', 'finished')
		end
		x.XPathHREFAll('//div[@class="chapterlist"]/table//td[@class="col1"]/a', MANGAINFO.ChapterLinks, MANGAINFO.ChapterNames)
		MANGAINFO.ChapterLinks.Reverse(); MANGAINFO.ChapterNames.Reverse()
		return no_error
	else
		return net_error
	end
end

function GetPageNumber()
	TASK.PageLinks.Clear()
	TASK.PageNumber=0
	local s=MaybeFillHost(MODULE.RootURL,URL):gsub('/$', '') .. '-1.html'
	if HTTP.GET(s) then
		x=CreateTXQuery(HTTP.Document)
		TASK.PageNumber=x.XPath('(//select[@id="page"])[1]/option').Count
		return true
	else
		return false
	end
end

function GetImageURL()
	local baseurl = MaybeFillHost(MODULE.RootURL,URL)
	local s = baseurl:gsub('/$','') .. '-' .. tostring(WORKID+1) .. '.html'
	HTTP.Headers.Values['Referer'] = baseurl
	if HTTP.GET(s) then
		x=CreateTXQuery(HTTP.Document)
		TASK.PageLinks[WORKID]=x.XPathString('//img[@id="comicpic"]/@src')
		return true
	else
		return false
	end
end

function GetNameAndLink()
	local s = '0-9'
	if MODULE.CurrentDirectoryIndex ~= 0 then
		s = ALPHA_LIST_UP:sub(MODULE.CurrentDirectoryIndex+1,MODULE.CurrentDirectoryIndex+1)
	end
	if HTTP.GET(MODULE.RootURL..'/category/' .. s .. '_'.. (URL + 1) .. '.html') then
		x=CreateTXQuery(HTTP.Document)
		v=x.XPath('//*[@class="booklist"]//span[@class="pagetor"]//text()')
		local i = 1
		for j=1,v.Count do
			v1 = v.Get(j)
			local tmp = tonumber(v1.ToString())
			if (tmp ~= nil) and (tmp > i) then i = tmp end
		end
		UPDATELIST.CurrentDirectoryPageNumber = i
		x.XPathHREFTitleAll('//*[@class="booklist"]/table//dl/dd/a[1]', LINKS, NAMES)
		return no_error
	else
		return net_problem
	end
end

function Init()
	function AddWebsiteModule(id, name, url, category)
		local m = NewWebsiteModule()
		m.ID               = id
		m.Name             = name
		m.RootURL          = url
		m.Category         = category
		m.TotalDirectory   = ALPHA_LIST_UP:len()
		m.LastUpdated      = 'February 19, 2018'
		m.OnGetInfo        = 'GetInfo'
		m.OnGetPageNumber  = 'GetPageNumber'
		m.OnGetNameAndLink = 'GetNameAndLink'
		m.OnGetImageURL    = 'GetImageURL'
	end
	AddWebsiteModule('28b00751dc4449b6b0ea67210bb21cca', 'WieManga', 'https://www.wiemanga.com', 'German')
	AddWebsiteModule('a4caba7e5d8b42b78f9191f6652e9b12', 'MangaRussia', 'http://www.mangarussia.com', 'Russian')
end
