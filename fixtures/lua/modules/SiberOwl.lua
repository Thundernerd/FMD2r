function getinfo()
	MANGAINFO.URL=MaybeFillHost(MODULE.RootURL, URL)
	if HTTP.GET(MANGAINFO.URL) then
		local x=CreateTXQuery(HTTP.Document)
		local s = x.XPathString('//script[contains(., "var description")]')
		MANGAINFO.Title = GetBetween('title="', '";', s)
		MANGAINFO.CoverLink = MaybeFillHost(MODULE.RootURL, GetBetween('imageUrl="', '";', s))
		MANGAINFO.Summary = GetBetween('description="', '";', s)
		s = GetBetween('chapString="', '";', s)
		x.ParseHTML(s)
		local v = x.XPath('//a')
		for i = 1, v.Count do
			local v1 = v.Get(i)
			MANGAINFO.ChapterLinks.Add(MANGAINFO.URL .. '/' .. v1.GetAttribute('href'))
			MANGAINFO.ChapterNames.Add(v1.ToString())
		end
		MANGAINFO.ChapterLinks.Reverse(); MANGAINFO.ChapterNames.Reverse()
		return no_error
	else
		return net_problem
	end
end

function getpagenumber()
	TASK.PageLinks.Clear()
	if HTTP.GET(MaybeFillHost(MODULE.RootURL, URL)) then
		local x=CreateTXQuery(HTTP.Document)
		local s = x.XPathString('//script[contains(., "imageUrls")]')
		x.ParseHTML(GetBetween('imageUrls =', ';', s))
		local v = x.XPath('json(*)()')
		for i = 1, v.Count do
			TASK.PageLinks.Add(MaybeFillHost(MODULE.RootURL, v.Get(i).ToString()))
		end
	else
		return false
	end
	return true
end

function getnameandlink()
	if HTTP.GET(MODULE.RootURL) then
		local x = CreateTXQuery(HTTP.Document)
		local v = x.XPath('//div[@class="index-mangas"]/div/a')
		for i = 1, v.Count do
			local v1 = v.Get(i)
			LINKS.Add(v1.GetAttribute('href'))
			NAMES.Add(x.XPathString('.//div[@class="index-manga-title"]', v1))
		end
		return no_error
	else
		return net_problem
	end
end

function Init()
	local m = NewWebsiteModule()
	m.ID = '201234a2c811487c8542fb7ec2c92b20'
	m.Name = 'SiberOwl'
	m.RootURL = 'http://siberowl.com'
	m.Category = 'English-Scanlation'
	m.LastUpdated='April 10, 2018'
	m.OnGetInfo='getinfo'
	m.OnGetPageNumber='getpagenumber'
	m.OnGetNameAndLink='getnameandlink'
end
