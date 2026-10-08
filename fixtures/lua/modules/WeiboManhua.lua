json = require "utils.json"

-- Filled in to get URL for comic info
local infoURL = 'http://apiwap.vcomic.com/wbcomic/comic/comic_show?comic_id=%s&_request_from=pc'
-- Filled in to get cover URL
local coverURL = 'https://img.manhua.weibo.com/%s'
-- Filled in to get chapter URL
local chapterURL = 'http://apiwap.vcomic.com/wbcomic/comic/comic_play?chapter_id=%s&_request_from=pc'
-- Filled in to get series page URL
local seriesURL = 'http://manhua.weibo.com/c/%s'

function GetInfo()
	MANGAINFO.URL = MaybeFillHost(MODULE.RootURL,URL)
	local id = URL:match('c/(%d+)')

	HTTP.Reset()
	HTTP.MimeType = 'application/json'
	if HTTP.POST(string.format(infoURL,id)) then
		local x = CreateTXQuery(HTTP.Document)
		local y = json.decode(HTTP.Document.ToString())
		MANGAINFO.Title = y.data.comic.name
		if(y.data.comic.hcover ~= '') then
			MANGAINFO.CoverLink = string.format(coverURL, y.data.comic.hcover):gsub("_s","_b")
		else
			MANGAINFO.CoverLink = y.data.comic.cover
		end
		MANGAINFO.Authors = y.data.comic.sina_nickname
		MANGAINFO.Summary = y.data.comic.description

		-- Populate genres list
		local genres = ''
		local category = x.XPath('json(*).data.comic_cate()')

		if category.Count > 0 then genres = y.data.comic_cate[1].cate_name end
		for i = 2, category.Count do
			local c = y.data.comic_cate[i].cate_name
			genres = genres .. ', ' .. c
		end
		MANGAINFO.Genres = genres

		local chapter = x.XPath('json(*).data.chapter_list()')
		for i = 1, chapter.Count do
			MANGAINFO.ChapterLinks.Add(string.format(chapterURL,x.XPathString('chapter_id',chapter.Get(i))))
			MANGAINFO.ChapterNames.Add(y.data.chapter_list[i].chapter_name)
		end

		return no_error
	else
		return net_problem
	end
end

function GetPageNumber()
	if HTTP.GET(MaybeFillHost('http://apiwap.vcomic.com/',URL)) then
		local x = CreateTXQuery(HTTP.Document)
		local pages = x.XPath('json(*).data.json_content.page()')
		for i = 1, pages.Count do
			TASK.PageLinks.Add(x.XPathString('mobileImgUrl',pages.Get(i)))
		end
		return true
	else
		return false
	end
end

function GetNameAndLink()
	-- based on assumption of <10,000 comics, rows num must be increased if this is exceeded (!2500 at time of writing)
	if(HTTP.GET'(https://apiwap.vcomic.com/wbcomic/comic/filter_result?page_num=1&rows_num=10000&cate_id=0&end_status=0&comic_pay_status=0&order=comic_read_num&_request_from=pc') then
		local x = CreateTXQuery(HTTP.Document)
		local y = json.decode(HTTP.Document.ToString())
		local list = x.XPath('json(*).data.data()')
		for i = 1, list.Count do
			LINKS.Add(string.format(seriesURL,x.XPathString('comic_id',list.Get(i))))
			NAMES.Add(y.data.data[i].comic_name)
		end
	else
		return net_problem
	end
end

function GetDirectoryPageNumber()
	if(HTTP.GET'(https://apiwap.vcomic.com/wbcomic/comic/filter_result?page_num=1&rows_num=10000&cate_id=0&end_status=0&comic_pay_status=0&order=comic_read_num&_request_from=pc') then
		local x = CreateTXQuery(HTTP.Document)
		PAGENUMBER = tonumber(x.XPathString('json(*).data.data.rows_total')) or 1
		return no_error
	else
		return net_problem
	end
end

function Init()
	local m = NewWebsiteModule()
	m.ID                       = 'e140b99353ea4fa88c82ff9d557bf9b6'
	m.Category                 = 'Raw'
	m.Name                     = 'WeiboManhua'
	m.RootURL                  = 'http://manhua.weibo.com'
	m.OnGetInfo                = 'GetInfo'
	m.OnGetPageNumber          = 'GetPageNumber'
	m.OnGetNameAndLink         = 'GetNameAndLink'
	m.OnGetDirectoryPageNumber = 'GetDirectoryPageNumber'
end
