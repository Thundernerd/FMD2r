function getinfo()
	MANGAINFO.URL=MaybeFillHost(MODULE.RootURL, URL)
	HTTP.Cookies.Values['has_js'] = '1'
	if HTTP.GET(MANGAINFO.URL) then
		local x = CreateTXQuery(HTTP.Document)
		if MANGAINFO.Title == '' then
			MANGAINFO.Title = string.gsub(x.XPathString('//h1[@class="page-header"]'), ' Manga$', '')
		end
		x.XPathHREFAll('//table[contains(@class,"chlist")]//tr/td[1]/a', MANGAINFO.ChapterLinks, MANGAINFO.ChapterNames)
		MANGAINFO.ChapterLinks.Reverse(); MANGAINFO.ChapterNames.Reverse()

		local s = x.XPathString('//script[contains(., "jQuery.extend(Drupal")]')
		s = GetBetween('settings,', ');', s)
		x.ParseHTML(s)
		local v = x.XPath('json(*)/authcacheP13nAjaxAssemblies')
		local summaryQuery = x.XPathString('./span.authcache-p13n-asm-field-node-body', v)
		local artistQuery = x.XPathString('./span.authcache-p13n-asm-field-node-field-artist', v)
		local authorQuery = x.XPathString('./span.authcache-p13n-asm-field-node-field-author', v)
		local genresQuery = x.XPathString('./span.authcache-p13n-asm-field-node-field-genres', v)
		local coverQuery = x.XPathString('./span.authcache-p13n-asm-field-node-field-image2', v)
		local statusQuery = x.XPathString('./span.authcache-p13n-asm-field-node-field-status', v)

		function getField(aurl, query)
			if aurl ~= '' then
				HTTP.Reset()
				HTTP.Headers.Values['X-Authcache'] = '1'
				if HTTP.XHR(MODULE.RootURL .. aurl) then
					local s = GetBetween(':"', '"}', HTTP.Document.ToString())
					x.ParseHTML(s:gsub('\\"', '"'):gsub('\\/', '/'))
					return x.XPathString(query)
				end
			end
			return ''
		end

		MANGAINFO.Summary   = getField(summaryQuery, '*')
		MANGAINFO.Authors   = getField(authorQuery, '//div[contains(@class, "field-item")]')
		MANGAINFO.Artists   = getField(artistQuery, '//div[contains(@class, "field-item")]')
		MANGAINFO.CoverLink = getField(coverQuery, '//img/@src')
		MANGAINFO.Genres    = getField(genresQuery, 'string-join(//a, ", ")')
		MANGAINFO.Status    = MangaInfoStatusIfPos(getField(statusQuery, '//div[contains(@class, "field-item")]'))

		return no_error
	else
		return net_problem
	end
end

function getpagenumber()
	if HTTP.GET(MaybeFillHost(MODULE.RootURL, URL)) then
		local x = CreateTXQuery(HTTP.Document)
		x.ParseHTML(GetBetween('settings,', ');', x.XPathString('//script[contains(., "jQuery.extend(Drupal")]')))
		x.XPathStringAll('json(*).showmanga.paths()', TASK.PageLinks)
	else
		return false
	end
	return true
end

function getnameandlink()
	local s = MODULE.RootURL .. '/directory'
	if URL ~= '0' then s = s .. '?page=' .. URL; end
	if HTTP.GET(s) then
		local x = CreateTXQuery(HTTP.Document)
		local i = 1
		local v = x.XPath('//ul[@class="pagination"]/li')
		for j = 1, v.Count do
			local v1 = v.Get(j)
			local x = tonumber(v1.ToString())
			if (x ~= nil) and (x > i) then i = x; end
		end
		UPDATELIST.CurrentDirectoryPageNumber = i - 1
		v = x.XPath('//table[contains(@class,"directory_list")]//tr/td[1]/a')
		for j = 1, v.Count do
			local v1 = v.Get(j)
			local s = v1.ToString()
			if string.match(s:upper(), ' MANGA$') ~= nil then
				s = s:sub(1, -7)
			end
			LINKS.Add(v1.GetAttribute('href'))
			NAMES.Add(s)
		end
		return no_error
	else
		return net_problem
	end
end

function Init()
	function AddWebsiteModule(id, site, url)
		local m=NewWebsiteModule()
		m.ID=id
		m.Category='English'
		m.Name=site
		m.RootURL=url
		m.OnGetInfo='getinfo'
		m.OnGetPageNumber='getpagenumber'
		m.OnGetNameAndLink='getnameandlink'
	end
	AddWebsiteModule('f999ff1dc0b648fb8861d11cc350975b', 'MangaTail', 'https://www.mangatail.me')
	AddWebsiteModule('1ddf6e055c034f5b8d4b4577eebc080b', 'MangaSail', 'https://www.mangasail.co')
end