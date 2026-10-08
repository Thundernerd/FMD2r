-- Get manga info and chapter list:
function getinfo()
	local j, v, x = nil

	MANGAINFO.URL = MaybeFillHost(MODULE.RootURL, URL)
	HTTP.Cookies.Values['contents_cid'] = string.gsub(URL, '/contents/', ''):gsub('/', '')
	HTTP.Cookies.Values['contents_detail_pg'] = '1000'
	if HTTP.GET(MANGAINFO.URL) then
		x = CreateTXQuery(HTTP.Document)
		MANGAINFO.CoverLink = MaybeFillHost(MODULE.RootURL, x.XPathString('//figure/img/@src'))
		MANGAINFO.Title = x.XPathString('//div[@class="Breadcrumb"]/ul/li/div[@class="current"]/text()')
		MANGAINFO.Summary = x.XPathString('//section[@id="workInfo"]/p')
		v = x.XPath('//section[@id="episodeWrap"]//li[@class="ListCard"]/a')
		for i = 1, v.Count do
			v1 = v.Get(i)
			MANGAINFO.ChapterLinks.Add(v1.GetAttribute('href'))
			MANGAINFO.ChapterNames.Add(x.XPathString('div[@class="ListCard-content"]/h3', v1) .. ' (' .. x.XPathString('div[@class="ListCard-info-nodot"]', v1) .. ')')
		end
		MANGAINFO.ChapterLinks.Reverse(); MANGAINFO.ChapterNames.Reverse()

		return no_error
	end
	return net_problem
end

-- Get page number and page container LINKS:
function getpagenumber()
	local x = nil

	if HTTP.GET(MaybeFillHost(MODULE.RootURL, URL)) then
		if HTTP.GET(MaybeFillHost(MODULE.RootURL, CreateTXQuery(HTTP.Document).XPathString('//div[@class="ViewerContainer"]/@data-URL'))) then
			x = CreateTXQuery(HTTP.Document)
			x.ParseHTML(GetBetween('{', '}', x.XPathString('//*')))
			x.XPathStringAll('json(*)()', TASK.PageLinks)
			for i = 0, TASK.PageLinks.Count do
				TASK.PageLinks[i] = MaybeFillHost(MODULE.RootURL, TASK.PageLinks[i])
			end
		else
			return false
		end

		return true
	end
	return false
end

-- Go through all directory pages and get NAMES and LINKS for manga entries:
function getnameandlink()
	local v, x = nil

	HTTP.Cookies.Values['contents_list_pg'] = '1000'
	if HTTP.GET(MODULE.RootURL .. '/contents/all') then
		x = CreateTXQuery(HTTP.Document)
		v = x.XPath('//li[@class="OblongCard--border"]/a')
		for i = 1, v.Count do
			v1 = v.Get(i)
			LINKS.Add(v1.GetAttribute('href'))
			NAMES.Add(x.XPathString('div/h3[@class="OblongCard-title"]', v1))
		end

		return no_error
	end
	return net_problem
end

-- Initialize module:
function Init()
	local m = NewWebsiteModule()
	m.ID='d2b5be6cd6bf417c8bacd8806aec4a9b'
	m.Category='Raw'
	m.Name='NewType'
	m.RootURL='https://comic.webnewtype.com'
	m.OnGetInfo='getinfo'
	m.OnGetPageNumber='getpagenumber'
	m.OnGetNameAndLink = 'getnameandlink'
end
