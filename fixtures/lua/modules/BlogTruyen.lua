function Init()
	local m = NewWebsiteModule()
	m.ID                         = 'c878e7780b404a959e63179568cb7ab3'
	m.Name                       = 'BlogTruyen'
	m.RootURL                    = 'https://blogtruyen.vn'
	m.Category                   = 'Vietnamese'
	m.OnGetDirectoryPageNumber   = 'GetDirectoryPageNumber'
	m.OnGetNameAndLink           = 'GetNameAndLink'
	m.OnGetInfo                  = 'GetInfo'
	m.OnGetPageNumber            = 'GetPageNumber'
	m.OnBeforeDownloadImage      = 'BeforeDownloadImage'
end

local dirurl = '/ajax/Search/AjaxLoadListManga?key=tatca&orderBy=1&p='

function GetDirectoryPageNumber()
	if HTTP.GET(MODULE.RootURL .. dirurl .. '1') then
		PAGENUMBER = tonumber(CreateTXQuery(HTTP.Document).XPathString('//*[@class="paging"]/span[last()]/a/@href'):match('%((.-)%)')) or 1
		return no_error
	else
		return net_problem
	end
end

function GetNameAndLink()
	if HTTP.GET(MODULE.RootURL .. dirurl .. (URL + 1)) then
		CreateTXQuery(HTTP.Document).XPathHREFAll('//*[@class="list"]//span/a', LINKS, NAMES)
		return no_error
	else
		return net_problem
	end
end

function GetInfo()
	MANGAINFO.URL = MaybeFillHost(MODULE.RootURL, URL)
	if HTTP.GET(MANGAINFO.URL) then
		local x = CreateTXQuery(HTTP.Document)

		MANGAINFO.CoverLink = x.XPathString('//*[@class="thumbnail"]/img/@src')
		MANGAINFO.Title     = x.XPathString('//title/substring-before(.," | BlogTruyen")')
		MANGAINFO.Authors   = x.XPathString('//*[@class="description"]/p[starts-with(.,"Tác giả")]/string-join(.//a,", ")')
		MANGAINFO.Genres    = x.XPathString('//*[@class="description"]/p[starts-with(.,"Thể loại")]/string-join(.//a,", ")')
		MANGAINFO.Summary   = x.XPathString('//*[@class="detail"]/*[@class="content"]')

		x.XPathHREFAll('//*[@id="list-chapters"]//span[@class="title"]/a', MANGAINFO.ChapterLinks, MANGAINFO.ChapterNames)
		MANGAINFO.ChapterLinks.Reverse(); MANGAINFO.ChapterNames.Reverse()
		return no_error
	else
		return net_problem
	end
end

function GetPageNumber()
	if HTTP.GET(MaybeFillHost(MODULE.RootURL, URL)) then
		local crypto = require 'fmd.crypto'
		CreateTXQuery(HTTP.Document).XPathStringAll('//article[@id="content"]/img[not(contains(@src,"credit"))]/@src', TASK.PageLinks)
		local i, image for i = 0, TASK.PageLinks.Count - 1 do
			image = crypto.DecodeURL(TASK.PageLinks[i])
			image = image:gsub('^.-[%&%?]url=', '')
			-- todo:
			TASK.PageLinks[i] = image
		end
		return true
	else
		return false
	end
end

function BeforeDownloadImage()
	HTTP.Headers.Values['Referer'] = MaybeFillHost(MODULE.RootURL, TASK.ChapterLinks[TASK.CurrentDownloadChapterPtr])
	return true
end
