function GetInfo()
	MANGAINFO.URL=MaybeFillHost(MODULE.RootURL,URL)
	if HTTP.GET(MANGAINFO.URL) then
		local x=CreateTXQuery(HTTP.Document)
		if MANGAINFO.Title == '' then
			MANGAINFO.Title=x.XPathString('css("div.title")')
		end
		MANGAINFO.CoverLink=MaybeFillHost(MODULE.RootURL, x.XPathString('css(".cover > img")/@src'))
		local status = x.XPathString('//li[@class="info_block" and contains(span, "Status")]/span[@class="info_content"]')
		MANGAINFO.Status = MangaInfoStatusIfPos(status, "In corso", "Completo")
		MANGAINFO.Authors = x.XPathString('//li[@class="info_block" and contains(span, "Autore")]/span[@class="info_content"]')
		MANGAINFO.Artists = x.XPathString('//li[@class="info_block" and contains(span, "Artista")]/span[@class="info_content"]')
		MANGAINFO.Genres = x.XPathString('//li[@class="info_block" and contains(span, "Genere")]/span[@class="info_content"]')
		MANGAINFO.Summary = x.XPathString('//div[@class="plot"]')
		x.XPathHREFAll('css("div.chapter_list > ul > li > .ch_top > a")', MANGAINFO.ChapterLinks, MANGAINFO.ChapterNames)
		MANGAINFO.ChapterLinks.Reverse(); MANGAINFO.ChapterNames.Reverse()
		return no_error
	else
		return net_problem
	end
end

function GetPageNumber()
	TASK.PageLinks.Clear()
	TASK.PageNumber = 0
	if HTTP.GET(MaybeFillHost(MODULE.RootURL, URL)) then
		local duktape = require 'fmd.duktape'
		local crypto = require 'fmd.crypto'
		local x=CreateTXQuery(HTTP.Document)
		local isExt = (x.XPathString('//script[contains(@src, "rext.js")]/@src') ~= '')
		local title=x.XPathString('//title')
		local s=x.XPathString('//script[contains(., "current_page")]')
		s=duktape.ExecJS(s .. ';JSON.stringify({m:m,ch:ch,chs:chs});')
		x.ParseHTML(s)
		local m=x.XPathString('json(*).m')
		local ch=x.XPathString('json(*).ch')
		local chs=x.XPathString('json(*).chs')
		local data=string.format('info[manga]=%s&info[chapter]=%s&info[ch_sub]=%s&info[title]=%s', m, ch, chs, title)
		if isExt then data = data .. '&info[external]=1' end
		HTTP.Reset()
		if HTTP.POST(MaybeFillHost(MODULE.RootURL, '/reader/c_i'), crypto.EncodeURL(data)) then
			x.ParseHTML(duktape.ExecJS(HTTP.Document.ToString()))
			local PATH = x.XPathString('json(*)()[3]')
			local v=x.XPath('json(*)()[2]()')
			local t={}
			for i = 1, v.Count do
				local v1 = v.Get(i)
				table.insert(t, v1.ToString())
			end
			v=x.XPath('json(*)()[1]()')
			for i = 1, v.Count do
				local v1=v.Get(i)
				if isExt then
					s = string.format('%s/%s%s', t[i], x.XPathString('./name', v1), x.XPathString('./ex', v1))
				else
					s = string.format('/reader/%s/%s%s%s', PATH, x.XPathString('./name', v1), t[i], x.XPathString('./ex', v1))
				end
				TASK.PageLinks.Add(MaybeFillHost(MODULE.RootURL, s))
			end
			return true
		end
	end
	return false
end

function GetNameAndLink()
	if HTTP.GET(MODULE.RootURL..'/reader/series') then
		local x=CreateTXQuery(HTTP.Document)
		x.XPathHREFAll('css(".manga_title > a")', LINKS, NAMES)
		return no_error
	else
		return net_problem
	end
end

function Init()
	local m = NewWebsiteModule()
	m.ID               = 'a0e4b47bbb0445b99ad9be9f2d34d47e'
	m.Category         = 'Italian-Scanlation'
	m.Name             = 'DigitalTeam'
	m.RootURL          = 'https://dgtread.com'
	m.OnGetInfo        = 'GetInfo'
	m.OnGetPageNumber  = 'GetPageNumber'
	m.OnGetNameAndLink = 'GetNameAndLink'
end
