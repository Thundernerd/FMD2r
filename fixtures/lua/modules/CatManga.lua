function Init()
	local m = NewWebsiteModule()
	m.ID                         = '37cd32b009d14737adce6ae784f5045d'
	m.Name                       = 'CatManga'
	m.RootURL                    = 'https://catmanga.org'
	m.Category                   = 'English'
	m.OnGetNameAndLink           = 'GetNameAndLink'
	m.OnGetInfo                  = 'GetInfo'
	m.OnGetPageNumber            = 'GetPageNumber'
end

function GetNameAndLink()
	if HTTP.GET(MODULE.RootURL) then
		local x = CreateTXQuery(HTTP.Document)
		local v for v in x.XPath('json(//script[@type="application/json"]).props.pageProps.series()').Get() do
			LINKS.Add('/series/' .. x.XPathString('series_id', v))
			NAMES.Add(x.XPathString('title', v))
		end
		return no_error
	else
		return net_problem
	end
end

function GetInfo()
	MANGAINFO.URL = MaybeFillHost(MODULE.RootURL, URL)
	if HTTP.GET(MANGAINFO.URL) then
		local x = CreateTXQuery(HTTP.Document)
		local minfo = x.XPath('json(//script[@type="application/json"]).props.pageProps.series')
		MANGAINFO.Title      = x.XPathString('title', minfo)
		MANGAINFO.CoverLink  = x.XPathString('cover_art/source', minfo)
		MANGAINFO.Authors    = x.XPathStringAll('json(//script[@type="application/json"]).props.pageProps.series.authors()')
		MANGAINFO.Genres     = x.XPathStringAll('json(//script[@type="application/json"]).props.pageProps.series.genres()')
		MANGAINFO.Status     = MangaInfoStatusIfPos(x.XPathString('props/pageProps/series/status', minfo))
		MANGAINFO.Summary    = x.XPathString('description', minfo)

		local series   = x.XPathString('series_id', minfo)
		local chapters = x.XPath('json(//script[@type="application/json"]).props.pageProps.series.chapters()')
		for ic = 1, chapters.Count do
			local title   = x.XPathString('title', chapters.Get(ic))
			local volume  = x.XPathString('volume', chapters.Get(ic))
			local chapter = x.XPathString('number', chapters.Get(ic))
			local group   = ' [' .. x.XPathString('json(//script[@type="application/json"]).props.pageProps.series.chapters()[' .. ic .. '].groups()') .. ']'

			if title ~= '' then title = ' - ' .. title end
			if volume ~= '' then volume = string.format('Vol. %s ', volume) end
			if chapter ~= '' then chapter = string.format('Ch. %s', chapter) end

			MANGAINFO.ChapterLinks.Add('/series/' .. series .. '/' .. x.XPathString('number', chapters.Get(ic)))
			MANGAINFO.ChapterNames.Add(volume .. chapter .. title .. group)
		end
		return no_error
	else
		return net_problem
	end
end

function GetPageNumber()
	if HTTP.GET(MaybeFillHost(MODULE.RootURL, URL)) then
		CreateTXQuery(HTTP.Document).XPathStringAll('json(//script[@type="application/json"]).props.pageProps.pages()', TASK.PageLinks)
		return true
	else
		return false
	end
end
