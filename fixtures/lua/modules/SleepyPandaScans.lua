
function Init()
	local m = NewWebsiteModule()
	m.ID               = '834bc6e9402640c8a5a739fedc999075'
	m.Category         = 'English-Scanlation'
	m.Name             = 'SleepyPandaScans'
	m.RootURL          = 'https://sleepypandascans.co'
	m.LastUpdated      = 'February 15, 2018'
	m.OnGetInfo        = 'GetInfo'
	m.OnGetPageNumber  = 'GetPageNumber'
	m.OnGetNameAndLink = 'GetNameAndLink'
end

function GetInfo()
	MANGAINFO.URL=MaybeFillHost(MODULE.RootURL,URL)
	if HTTP.GET(MANGAINFO.URL) then
		x=CreateTXQuery(HTTP.Document)
		MANGAINFO.Title=x.XPathString('//h4[contains(@class, "card-title")]')
		MANGAINFO.CoverLink=MaybeFillHost(MODULE.RootURL, x.XPathString('//img[contains(@class, "card-img-top")]/@src'))
		MANGAINFO.Summary = x.XPathString('//p[contains(@class, "card-text")]')
		v=x.XPath('//div[contains(@class, "list-group")]/a')
		for i=1,v.Count do
			v1=v.Get(i)
			MANGAINFO.ChapterLinks.Add(v1.GetAttribute('href'))
			MANGAINFO.ChapterNames.Add(x.XPathString('./text()', v1))
		end
		MANGAINFO.ChapterLinks.Reverse(); MANGAINFO.ChapterNames.Reverse()
		return no_error
	else
		return net_problem
	end
end

function GetPageNumber()
	TASK.PageLinks.Clear()
	TASK.PageNumber=0
	if HTTP.GET(MaybeFillHost(MODULE.RootURL,URL)) then
		local x=CreateTXQuery(HTTP.Document)
		local v = x.XPath('//div[@class="view"]/img')
		for i=1,v.Count do
			v1=v.Get(i)
			TASK.PageLinks.Add(MODULE.RootURL..v1.GetAttribute('src'))
		end
		return true
	else
		return false
	end
end

function GetNameAndLink()
	if HTTP.GET(MODULE.RootURL..'/series') then
		x=CreateTXQuery(HTTP.Document)
		x.XPathStringAll('//h6[@class="card-title"]', NAMES)
		x.XPathStringAll('//div[@class="container"]//div[contains(@class,"card")]//div/a/@href', LINKS)
		return no_error
	else
		return net_problem
	end
end