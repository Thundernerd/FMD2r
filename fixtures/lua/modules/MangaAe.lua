local dirurl='/manga'

function GetDirectoryPageNumber()
	PAGENUMBER=1
	if HTTP.GET(MODULE.RootURL..dirurl) then
		PAGENUMBER=CreateTXQuery(HTTP.Document).XPathCount('//div[@class="pagination"]/a')
		return no_error
	else
		return net_error
	end
end

function GetNameAndLink()
	if HTTP.GET(MODULE.RootURL..dirurl..'/page:'..(URL + 1)) then
			CreateTXQuery(HTTP.Document).XPathHREFAll('//div[@id="mangadirectory"]/div[@class="mangacontainer"]/a[2]', LINKS, NAMES)
		return no_error
	else
		return net_problem
	end
end

function GetInfo()
	MANGAINFO.URL=MaybeFillHost(MODULE.RootURL,URL)
	if HTTP.GET(MANGAINFO.URL) then
		x=CreateTXQuery(HTTP.Document)
		MANGAINFO.Title=x.XPathString('//h1[@class="EnglishName"]'):match('^%((.*)%)$')
		MANGAINFO.CoverLink=x.XPathString('//img[@class="manga-cover"]/resolve-uri(@src)')
		MANGAINFO.Authors=x.XPathString('//div[@class="manga-details-author"]/h4[1]')
		MANGAINFO.Genres=x.XPathString('//div[@class="manga-details-extended"]/ul/string-join(./li/a,", ")')
		MANGAINFO.Status=MangaInfoStatusIfPos(x.XPathString('//div[@class="manga-details-extended"]/h4[2]'),
					'مستمرة',
					'مكتملة')
		x.XPathHREFAll('//ul[@class="new-manga-chapters"]/li/a', MANGAINFO.ChapterLinks, MANGAINFO.ChapterNames)
		MANGAINFO.ChapterLinks.Reverse(); MANGAINFO.ChapterNames.Reverse()
		return no_error
	else
		return net_problem
	end
end

function GetPageNumber()
	local u=MODULE.RootURL..URL:gsub('/*$','')
	if u:match('(%d+)$')=='1' then u=u:gsub('/1$','') end
	u=u..'/0/full'
	if HTTP.GET(u) then
		CreateTXQuery(HTTP.Document).XPathStringAll('//*[@id="showchaptercontainer"]//img/resolve-uri(@src)', TASK.PageLinks)
		return true
	else
		return false
	end
end

function Init()
	local m = NewWebsiteModule()
	m.ID = '5366eb91e0394d04892884b2705035d1'
	m.Name='MangaAe'
	m.RootURL='https://manga.ae'
	m.Category='Arabic'
	m.OnGetDirectoryPageNumber='GetDirectoryPageNumber'
	m.OnGetNameAndLink='GetNameAndLink'
	m.OnGetInfo='GetInfo'
	m.OnGetPageNumber='GetPageNumber'
end
