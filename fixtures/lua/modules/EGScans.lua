function getinfo()
		local lurl = MaybeFillHost(MODULE.RootURL, URL)
		local result = net_problem
		if HTTP.GET(lurl) then
				local x = CreateTXQuery(HTTP.Document)
				if MANGAINFO.Title == '' then
						MANGAINFO.Title = x.XPathString('//select[@name="manga"]/option[@selected]')
				end
				local v = x.XPath('//select[@name="chapter"]/option')
				for i = 1, v.Count do
						local v1 = v.Get(i)
						MANGAINFO.ChapterNames.Add(v1.ToString())
						MANGAINFO.ChapterLinks.Add(URL .. '/' .. v1.GetAttribute('value'))
				end
				result = no_error
		end
		return result
end

function taskstart()
		TASK.PageLinks.Clear()
		TASK.PageNumber = 0
		return true
end

function getpagenumber()
		local result = false
		local s = URL .. '&display=webtoon'
		s = MaybeFillHost(MODULE.RootURL, s)
		if HTTP.GET(s) then
				local x = CreateTXQuery(HTTP.Document)
				local v = x.XPath('//td/a/img[@class="picture"]/@src')
				for i = 1, v.Count do
						local v1 = v.Get(i)
						TASK.PageLinks.Add(MaybeFillHost(MODULE.RootURL, v1.ToString()))
				end
				result = true
		end
		return result
end

function getnameandlink()
		local result = net_problem
		if HTTP.GET(MODULE.RootURL) then
				local x = CreateTXQuery(HTTP.Document)
				local v = x.XPath('//select[@name="manga"]/option[@value!="0"]')
				for i = 1, v.Count do
						local v1 = v.Get(i)
						LINKS.Add(MODULE.RootURL .. '/' .. v1.GetAttribute('value'))
						NAMES.Add(v1.ToString())
				end
				result = no_error
		end
		return result
end

function Init()
		local m = NewWebsiteModule()
	m.ID = 'bf1790519ae0467792a13d58271e0116'
		m.Category = 'English-Scanlation'
		m.Name = 'EGScans'
		m.RootURL = 'http://read.egscans.com'
		m.LastUpdated = 'february, 10 2018'
		m.OnGetInfo = 'getinfo'
		m.OnTaskStart = 'taskstart'
		m.OnGetPageNumber = 'getpagenumber'
		m.OnGetNameAndLink = 'getnameandlink'
end