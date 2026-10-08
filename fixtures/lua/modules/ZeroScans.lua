----------------------------------------------------------------------------------------------------
-- Module Initialization
----------------------------------------------------------------------------------------------------

function Init()
	local m = NewWebsiteModule()
	m.ID                       = 'a0505ce2903c4903b398eaabb5cf1d72'
	m.Name                     = 'ZeroScans'
	m.RootURL                  = 'https://zscans.com'
	m.Category                 = 'English-Scanlation'
	m.OnGetNameAndLink         = 'GetNameAndLink'
	m.OnGetInfo                = 'GetInfo'
	m.OnGetPageNumber          = 'GetPageNumber'

	local fmd = require 'fmd.env'
	local slang = fmd.SelectedLanguage
	local lang = {
		['en'] = {
			['imagequality'] = 'Image quality:',
			['quality'] = 'High quality\nGood quality'
		},
		['id_ID'] = {
			['imagequality'] = 'Kualitas gambar:',
			['quality'] = 'Kualitas tinggi\nKualitas bagus'
		},
		get =
			function(self, key)
				local sel = self[slang]
				if sel == nil then sel = self['en'] end
				return sel[key]
			end
	}
	m.AddOptionComboBox('imagequality', lang:get('imagequality'), lang:get('quality'), 0)
end

----------------------------------------------------------------------------------------------------
-- Event Functions
----------------------------------------------------------------------------------------------------

-- Get links and names from the manga list of the current website.
function GetNameAndLink()
	if not HTTP.GET(MODULE.RootURL .. '/swordflake/comics') then return net_problem end

	local x = CreateTXQuery(HTTP.Document)
	local v for v in x.XPath('json(*).data.comics()').Get() do
		LINKS.Add('comics/' .. x.XPathString('slug', v))
		NAMES.Add(x.XPathString('name', v))
	end

	return no_error
end

-- Get info and chapter list for current manga.
function GetInfo()
	if not HTTP.GET(MaybeFillHost(MODULE.RootURL, URL)) then return net_problem end

	local x = CreateTXQuery(HTTP.Document)
	local duktape = require 'fmd.duktape'
	local s = x.XPathString('//script[contains(., "__ZEROSCANS__")]'):gsub('window.', '')
	x.ParseHTML(duktape.ExecJS(s .. ';JSON.stringify(__ZEROSCANS__);'))
	MANGAINFO.Title     = x.XPathString('json(*).data().details.name')
	MANGAINFO.CoverLink = x.XPathString('json(*).data().details.cover.full')
	MANGAINFO.Genres    = x.XPathStringAll('json(*).data().details.genres().name')
	MANGAINFO.Summary   = x.XPathString('json(*).data().details.summary')
	local status = x.XPathString('json(*).data().details.statuses().name[1]')
	if (status == 'Ongoing') or (status == 'New') or (status == 'Hiatus') then
		status = 'ongoing'
	else
		status = 'completed'
	end
	MANGAINFO.Status = MangaInfoStatusIfPos(status)

	local mangaSlug = x.XPathString('json(*).data().details.slug')
	local firstchapId = x.XPathString('json(*).data().details.first_chapter().id')
	if HTTP.GET(MODULE.RootURL .. '/swordflake/comic/' .. mangaSlug .. '/chapters/' .. firstchapId) then
		local x = CreateTXQuery(HTTP.Document)
		local v for v in x.XPath('json(*).data.chapters()').Get() do
			MANGAINFO.ChapterLinks.Add('swordflake/comic/' .. mangaSlug .. '/chapters/' .. v.GetProperty('id').ToString())
			MANGAINFO.ChapterNames.Add('Chapter ' .. v.GetProperty('name').ToString())
		end
	end

	return no_error
end

-- Get the page count for the current chapter.
function GetPageNumber()
	if not HTTP.GET(MaybeFillHost(MODULE.RootURL, URL)) then return net_problem end

	local imagequality = {'high_quality', 'good_quality'}
	local sel_imagequality = (MODULE.GetOption('imagequality') or 0) + 1
	local x = CreateTXQuery(HTTP.Document)
	local v for v in x.XPath('json(*).data.chapter.' .. imagequality[sel_imagequality] .. '()').Get() do
		TASK.PageLinks.Add(v.ToString())
	end

	return no_error
end
