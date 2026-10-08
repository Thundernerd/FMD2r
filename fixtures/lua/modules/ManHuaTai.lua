function getinfo()
	MANGAINFO.URL=MaybeFillHost(MODULE.RootURL, URL)
	if HTTP.GET(MANGAINFO.URL) then
		x=CreateTXQuery(HTTP.Document)
		MANGAINFO.Title=x.XPathString('//div[contains(@class, "mhjsbody")]/div/ul/li[contains(., "名称")]/substring-after(., "名称：")')
		MANGAINFO.CoverLink=MaybeFillHost(MODULE.RootURL,x.XPathString('//div[@class="comic-cover"]/img/@src'))
		MANGAINFO.Authors=x.XPathString('//div[contains(@class, "mhjsbody")]/div/ul/li[contains(., "作者")]/substring-after(., "作者：")')
		MANGAINFO.Genres=x.XPathString('//div[contains(@class, "mhjsbody")]/div/ul/li[contains(., "类型")]/substring-after(., "类型：")')
		MANGAINFO.Status = MangaInfoStatusIfPos(x.XPathString('//div[contains(@class, "mhjsbody")]/div/ul/li[contains(., "状态")]/substring-after(., "状态：")'), '连载至', '已完结');
		MANGAINFO.Summary=x.XPathStringAll('//div[contains(@class,"wz")]/div/text()', '')
		v=x.XPath('//div[@class="mhlistbody"]/ul')
		for i=v.Count,1,-1 do
			v1=v.Get(i)
			w = x.XPath('./li/a', v1)
			for j = 1, w.Count do
				w1 = w.Get(j)
				MANGAINFO.ChapterLinks.Add(MANGAINFO.URL .. '/' .. w1.GetAttribute('href'))
				MANGAINFO.ChapterNames.Add(w1.GetAttribute('title'))
			end
		end
		MANGAINFO.ChapterLinks.Reverse(); MANGAINFO.ChapterNames.Reverse()
		return no_error
	else
		return net_problem
	end
end

function getpagenumber()
	local servers = {
		'http://mhpic.mh51.com',
		'http://mhpic.manhualang.com',
		'http://mhpic.jumanhua.com',
		'http://mhpic.yyhao.com',
	}

	math.randomseed(os.time())
	math.random(); math.random(); math.random();

	if HTTP.GET(MaybeFillHost(MODULE.RootURL,URL)) then
		x=CreateTXQuery(HTTP.Document)
		local s = x.XPathString('//script[contains(., "mh_info")]')
		local imgpath = GetBetween('imgpath:"', '",', s)
		imgpath = imgpath:gsub('\\\\', '\\'):gsub("\\'", "'"):gsub('\\"', '"')
		local pageid = tonumber(s:match('pageid:%s*(%d+)'))
		local start = tonumber(s:match('startimg:%s*(%d+)'))
		local total = tonumber(s:match('totalimg:%s*(%d+)'))
		local size = GetBetween('comic_size:"', '",', s)
		imgpath = imgpath:gsub('(.)',
			function (a)
				return string.char(string.byte(a) - pageid % 10)
			end
		)
		local srv = servers[math.random(#servers)]
		for i=start,total do
			local d = tostring(start+i-1) .. '.jpg' .. size
			TASK.PageLinks.Add(srv .. '/comic/' .. imgpath .. d)
		end
		return true
	else
		return false
	end
end

function BeforeDownloadImage()
	HTTP.Headers.Values['Referer'] = MODULE.RootURL
	return true
end

function getdirectorypagenumber()
	if HTTP.GET(MODULE.RootURL .. '/all.html') then
		x = CreateTXQuery(HTTP.Document)
		PAGENUMBER = tonumber(x.XPathString('//div[@class="pages"]/a[last()-1]')) or 1
		return no_error
	else
		return net_problem
	end
end

function getnameandlink()
	if HTTP.GET(MODULE.RootURL..'/all_p'..(URL + 1)..'.html') then
		CreateTXQuery(HTTP.Document).XPathHREFTitleAll('//a[contains(div/ul/li/@class, "title")]',LINKS,NAMES)
		return no_error
	else
		return net_problem
	end
end

function Init()
	local m = NewWebsiteModule()
	m.ID = '68b46b326b77457091dd902300b0a31f'
	m.Category='Raw'
	m.Name='ManHuaTai'
	m.RootURL='http://www.manhuatai.com'
	m.LastUpdated='February 21, 2018'
	m.OnGetInfo='getinfo'
	m.OnGetPageNumber='getpagenumber'
	m.OnGetDirectoryPageNumber='getdirectorypagenumber'
	m.OnGetNameAndLink='getnameandlink'
	m.OnBeforeDownloadImage = 'BeforeDownloadImage'
end