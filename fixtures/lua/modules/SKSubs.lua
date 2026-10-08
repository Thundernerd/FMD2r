function getinfo()
	MANGAINFO.URL=MaybeFillHost(MODULE.RootURL,URL)
	if HTTP.GET(MANGAINFO.URL) then
		x=CreateTXQuery(HTTP.Document)
		MANGAINFO.CoverLink=MaybeFillHost(MODULE.RootURL,x.XPathString('//div[contains(@class,"thumbnail")]/div[1]/img/@src'))
		MANGAINFO.Title=x.XPathString('//div[contains(@class,"thumbnail")]/div[2]/div[1]/h1')
		MANGAINFO.Genres=x.XPathString('//div[contains(@class,"thumbnail")]/div[2]/div[1]/string-join(./a,", ")')
		MANGAINFO.Summary=x.XPathString('//div[contains(@class,"thumbnail")]/div[2]/div[1]/p/substring-after(.,"Sinopsis: ")')
		local lurl=''
		while true do
			x.XPathStringAll('//ul[contains(@class,"list-group")]/a/@href',MANGAINFO.ChapterLinks)
			x.XPathStringAll('//ul[contains(@class,"list-group")]/a/text()[1]',MANGAINFO.ChapterNames)
			lurl=x.XPathString('//ul[@class="pagination"]/li[@class="active"]/following-sibling::li[not(@class)]/a/@href')
			if HTTP.Terminated then break end;
			if lurl~='' then
				if HTTP.GET(MaybeFillHost(MODULE.RootURL,lurl)) then
					x.ParseHTML(HTTP.Document)
				else
					break
				end
			else
				break
			end
		end
		MANGAINFO.ChapterLinks.Reverse(); MANGAINFO.ChapterNames.Reverse()
		return no_error
	else
		return net_problem
	end
end

function getpagenumber()
	if HTTP.GET(MaybeFillHost(MODULE.RootURL,URL)) then
		x=CreateTXQuery(HTTP.Document)
		TASK.PageNumber=tonumber(x.XPathString('//ul[@class="pagination"]/li[not(./a/@rel)][last()]'))
		return true
	else
		return false
	end
end

function getimageurl()
	local lurl=MaybeFillHost(MODULE.RootURL,URL)
	if WORKID~=0 then lurl=lurl..'/?page='..WORKID+1 end
	if HTTP.GET(lurl) then
		x=CreateTXQuery(HTTP.Document)
		TASK.PageLinks[WORKID]=MaybeFillHost(MODULE.RootURL,x.XPathString('//div[@class="card-body"]//img/@src'))
		return true
	else
		return false
	end
end

function getnameandlink()
	if HTTP.GET(MODULE.RootURL..'/novelas') then
		x=CreateTXQuery(HTTP.Document)
		x.XPathHREFAll('//div[contains(@class,"thumbnail")]/div[2]/div[1]/a[1]', LINKS, NAMES)
		return no_error
	else
		return net_problem
	end
end

function Init()
	local m = NewWebsiteModule()
	m.ID = 'f7dd3ba012ae44b2a6fb1fa917c03498'
	m.Category='Spanish-Scanlation'
	m.Name='SKSubs'
	m.RootURL='http://sksubs.com'
	m.LastUpdated='February 28, 2018'
	m.OnGetInfo='getinfo'
	m.OnGetPageNumber='getpagenumber'
	m.OnGetImageURL='getimageurl'
	m.OnGetNameAndLink='getnameandlink'
end
