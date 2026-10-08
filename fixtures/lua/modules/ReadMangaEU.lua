function getinfo()
	MANGAINFO.URL=MaybeFillHost(MODULE.RootURL, URL)
	if HTTP.GET(MANGAINFO.URL) then
		local x=CreateTXQuery(HTTP.Document)
		MANGAINFO.Title=x.XPathString('//h1[@class="ebook_title"]')
		MANGAINFO.CoverLink=MaybeFillHost(MODULE.RootURL, x.XPathString('//img[contains(@class, "ebook_cover")]/@src'))
		MANGAINFO.Authors=x.XPathString('//table[@class="details_table"]/tbody/tr[contains(td, "Author")]/td[2]')
		MANGAINFO.Genres=x.XPathStringAll('//table[@class="details_table"]/tbody/tr[contains(td, "Genres")]/td[2]/a')
		MANGAINFO.Status = MangaInfoStatusIfPos(x.XPathString('//table[@class="details_table"]/tbody/tr[contains(td, "Status")]/td[2]'))
		MANGAINFO.Summary=x.XPathString('//*[@class="ebook_description"]')
		x.XPathHREFAll('//div[contains(@class,"chapters")]/span[not(@id="show_all")]/a', MANGAINFO.ChapterLinks, MANGAINFO.ChapterNames)
		MANGAINFO.ChapterLinks.Reverse(); MANGAINFO.ChapterNames.Reverse()
		return no_error
	else
		return net_problem
	end
end

function getpagenumber()
	TASK.PageLinks.Clear()
	TASK.PageNumber=0
	if HTTP.GET(MaybeFillHost(MODULE.RootURL, URL)) then
		TASK.PageNumber=CreateTXQuery(HTTP.Document).XPathCount('//select[@id="jumpto"]/option')
	else
		return false
	end
	return true
end

function getnameandlink()
	if HTTP.GET(MODULE.RootURL .. '/manga-list') then
		local x = CreateTXQuery(HTTP.Document)
		x.XPathHREFAll('//ul[@class="ul_list"]/li/a', LINKS, NAMES)
		return no_error
	else
		return net_problem
	end
end

function getimageurl()
	if HTTP.GET(MaybeFillHost(MODULE.RootURL,URL):gsub('%d+$',tostring(WORKID+1))) then
		local x = CreateTXQuery(HTTP.Document)
		local s = x.XPathString('//img[contains(@class, "ebook_img")]/@src')
		TASK.PageLinks[WORKID] = MaybeFillHost(MODULE.RootURL, s)
		return true
	end
	return false
end

function Init()
	local m = NewWebsiteModule()
	m.ID = '897daf6de489476cb26993ca89c93c8e'
	m.Name = 'ReadMangaEU'
	m.RootURL = 'http://www.readmanga.eu'
	m.Category = 'English'
	m.LastUpdated='March 1, 2018'
	m.OnGetInfo='getinfo'
	m.OnGetPageNumber='getpagenumber'
	m.OnGetNameAndLink='getnameandlink'
	m.OnGetImageURL='getimageurl'
end
