function getinfo()
	MANGAINFO.URL=MaybeFillHost(MODULE.RootURL, URL)
	if HTTP.GET(MANGAINFO.URL) then
		local x=CreateTXQuery(HTTP.Document)
		MANGAINFO.Title=x.XPathString('//div[@class="con"]/h2')
		MANGAINFO.CoverLink=MaybeFillHost(MODULE.RootURL, x.XPathString('//*[@class="bg_img_small"]/img/@src'))
		MANGAINFO.Authors=x.XPathStringAll('//div[@class="con"]/dl/dd[@class="name"]/span[@class="aln"]')
		MANGAINFO.Genres=x.XPathString('//div[@class="con"]/dl/dd[@class="name"]/p'):gsub('%s+', ''):gsub(',', ', ')
		MANGAINFO.Summary=x.XPathString('//div[@class="con"]/dl/dd[@class="dsc"]')
		if HTTP.GET(MANGAINFO.URL .. '/chapters') then
			x.ParseHTML(HTTP.Document)
			local v = x.XPath('json(*).data.list()')
			for i = 1, v.Count do
				local v1 = v.Get(i)
				MANGAINFO.ChapterLinks.Add(MANGAINFO.URL .. '/chapters/' .. x.XPathString('id', v1))
				local s = x.XPathString('name', v1)
				if x.XPathString('./salePolicy/isFree', v1) == 'false' then
					s = s .. ' [LOCKED]'
				end
				MANGAINFO.ChapterNames.Add(s)
			end
		end
		return no_error
	else
		return net_problem
	end
end

function getpagenumber()
	TASK.PageLinks.Clear()
	if HTTP.GET(MaybeFillHost(MODULE.RootURL, URL)) then
		x=CreateTXQuery(HTTP.Document)
		local function HexToStr(str)
			return str:gsub('%x%x',function(c)return c.char(tonumber(c,16))end)
		end
		json = HexToStr(GetBetween('keyList : "', '"', x.XPathString('//script[contains(., "keyList")]')))
		x.ParseHTML(json)
		x.XPathStringAll('json(*).list().URL', TASK.PageLinks)
	else
		return false
	end
	return true
end

local dirurls = {
	'/weekly/list?page=',
	'/titles/completed?page='
}

function getnameandlink()
	local lurl = dirurls[MODULE.CurrentDirectoryIndex+1]
	if HTTP.GET(MODULE.RootURL .. lurl .. (URL + 1)) then
		local x = CreateTXQuery(HTTP.Document)
		local s = 'json(*).data().list()'
		if MODULE.CurrentDirectoryIndex == 1 then
			s = 'json(*).data.list()'
		end
		local v = x.XPath(s)
		local hasTitles = false
		for i = 1, v.Count do
			local v1 = v.Get(i)
			LINKS.Add(MODULE.RootURL..'/titles/'..x.XPathString('id', v1))
			NAMES.Add(x.XPathString('name', v1))
			hasTitles = true
		end
		if hasTitles then
			UPDATELIST.CurrentDirectoryPageNumber = UPDATELIST.CurrentDirectoryPageNumber + 1
		end
	end
	return net_problem
end

function Init()
	local m = NewWebsiteModule()
	m.ID = 'ac485e1342de4c548d5b5c42c043a9d7'
	m.Name = 'ComicoCoID'
	m.RootURL = 'http://www.comico.co.ID'
	m.Category = 'Indonesian'
	m.OnGetInfo='getinfo'
	m.OnGetPageNumber='getpagenumber'
	m.OnGetNameAndLink='getnameandlink'
	m.TotalDirectory = 2
end
