local dirpages = {
	'magic', 'isekai', 'school-life', 'supernatural', 'historical', 'school', 'yuri', 'martial-arts', 'shounen', 'psychological',
	'romance', 'aventure', 'horror', 'harem', 'drama', 'sport', 'smut', 'mystery', 'sports', 'comedy',
	'adventure', 'updating', 'action', 'yaoi', 'tragedy', 'shoujo-ai', 'fantasy', 'slice-of-life', 'shoujo', 'seinen',
	'sci-fi', 'school-lifi', 'mecha', 'mature', 'josei', 'hot-ecchi', 'ecchi', 'adult'
	}

function Init()
	local m = NewWebsiteModule()
	m.ID                       = '2edfd16aed564b79afd84011745f1670'
	m.Category                 = 'Raw'
	m.Name                     = 'ComickTop'
	m.RootURL                  = 'https://comick.top'
	m.OnGetNameAndLink         = 'GetNameAndLink'
	m.OnGetInfo                = 'GetInfo'
	m.OnGetPageNumber          = 'GetPageNumber'
	m.OnBeforeDownloadImage    = 'BeforeDownloadImage'
	m.TotalDirectory           = #dirpages
end

function GetNameAndLink()
	if HTTP.GET(MODULE.RootURL .. '/genre/' .. dirpages[MODULE.CurrentDirectoryIndex + 1] .. '?page=' .. (URL + 1)) then
		local x = CreateTXQuery(HTTP.Document)
		x.XPathHREFTitleAll('//h3[@class="truyen-title"]/a', LINKS, NAMES)
		UPDATELIST.CurrentDirectoryPageNumber = tonumber(x.XPathString('//ul[@class="pagination"]/li[last()-2]/a')) or 1
		return no_error
	else
		return net_problem
	end
end

function GetInfo()
	MANGAINFO.URL = MaybeFillHost(MODULE.RootURL, URL)
	if HTTP.GET(MANGAINFO.URL) then
		local x = CreateTXQuery(HTTP.Document)
		MANGAINFO.Title      = x.XPathString('//h3[@class="title"]')
		MANGAINFO.CoverLink  = x.XPathString('//div[@id="item-intro"]/img/@data-src')
		MANGAINFO.Authors    = x.XPathStringAll('//div[@class="info_story"]//a[@itemprop="author"]')
		MANGAINFO.Genres     = x.XPathStringAll('//div[@class="info_story"]//a[@itemprop="genre"]')
		MANGAINFO.Summary    = x.XPathString('//div[@itemprop="description"]')
		local p = 1
		local pages = tonumber(x.XPathString('//ul[@class="pagination"]/li[last()-2]/a'))
		if pages == nil then pages = 1 end
		while true do
			x.XPathHREFTitleAll('//ul[@class="list-chapter"]//a', MANGAINFO.ChapterLinks, MANGAINFO.ChapterNames)
			p = p + 1
			if p > pages then
				break
			elseif HTTP.GET(MaybeFillHost(MODULE.RootURL, URL .. '?page=' .. tostring(p))) then
				x.ParseHTML(HTTP.Document)
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

function GetPageNumber()
	if HTTP.GET(MaybeFillHost(MODULE.RootURL, URL)) then
		CreateTXQuery(HTTP.Document).XPathStringAll('//div[@class="chapter-c"]/img/@data-src', TASK.PageLinks)
		return true
	else
		return false
	end
end

function BeforeDownloadImage()
	HTTP.Headers.Values['Referer'] = MaybeFillHost(MODULE.RootURL, TASK.ChapterLinks[TASK.CurrentDownloadChapterPtr])
	return true
end
