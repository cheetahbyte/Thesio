package library

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"time"

	"github.com/cheetahbyte/thesio/internal/vault"
	"gopkg.in/yaml.v3"
)

// Looks up bibliographic metadata by identifier: DOIs and arXiv IDs through
// doi.org, PMIDs through NCBI, ISBNs through Open Library. Also downloads
// open-access papers from arXiv and the locations Unpaywall lists.

// AppVersion goes into the User-Agent services see.
const AppVersion = "0.1.0"

const (
	userAgent    = "Thesio/" + AppVersion
	timeout      = 20 * time.Second
	paperTimeout = 120 * time.Second
	paperLimit   = 200 * 1024 * 1024
)

// ErrNotFound is a record the service does not have.
var ErrNotFound = errors.New("not found")

// Errors of lookups and downloads, matched with errors.Is. Details follow their text.
var (
	ErrStatus                  = errors.New("the service answered with status")
	ErrUnpaywallNoRecord error = message("Unpaywall returned no record")
	ErrNotWebLink              = errors.New("not a web link")
	ErrPaperTooLarge           = errors.New("the paper is larger than 200 MB")
	ErrNotPDF                  = errors.New("the link did not lead to a PDF")
	ErrUnknownIdentifier       = errors.New("unknown kind of identifier")
)

// closeBody closes a response body, adding its error to *err.
func closeBody(body io.Closer, err *error) {
	if cerr := body.Close(); cerr != nil {
		*err = errors.Join(*err, fmt.Errorf("lookup: %w", cerr))
	}
}

func request(ctx context.Context, url, accept string, wait time.Duration) (*http.Response, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, url, nil)
	if err != nil {
		return nil, fmt.Errorf("lookup: %w", err)
	}
	req.Header.Set("Accept", accept)
	req.Header.Set("User-Agent", userAgent)
	response, err := (&http.Client{Timeout: wait}).Do(req)
	if err != nil {
		return nil, fmt.Errorf("lookup: %w", err)
	}
	if response.StatusCode >= 400 {
		err = fmt.Errorf("%w %d", ErrStatus, response.StatusCode)
		if response.StatusCode == http.StatusNotFound {
			err = ErrNotFound
		}
		closeBody(response.Body, &err)
		return nil, err
	}
	return response, nil
}

func fetchText(ctx context.Context, url, accept string) (text string, err error) {
	response, err := request(ctx, url, accept, timeout)
	if err != nil {
		return "", err
	}
	defer closeBody(response.Body, &err)
	body, err := io.ReadAll(response.Body)
	if err != nil {
		return "", fmt.Errorf("lookup: %w", err)
	}
	return string(body), nil
}

// encode escapes what a path segment of a DOI cannot carry as it is.
func encodePath(id string) string {
	var out strings.Builder
	for i := 0; i < len(id); i++ {
		b := id[i]
		if 'A' <= b && b <= 'Z' || 'a' <= b && b <= 'z' || '0' <= b && b <= '9' || strings.IndexByte("-._~/():;", b) >= 0 {
			out.WriteByte(b)
		} else {
			fmt.Fprintf(&out, "%%%02X", b)
		}
	}
	return out.String()
}

func fetchDOI(ctx context.Context, doi string) (*yaml.Node, error) {
	body, err := fetchText(ctx, "https://doi.org/"+encodePath(doi), "application/x-bibtex")
	if err != nil {
		return nil, err
	}
	entry, err := record(body)
	if err != nil {
		return nil, err
	}
	// The URL only restates the DOI.
	if strings.Contains(read(entry, URL.Slot), "doi.org/") {
		write(entry, URL.Slot, "")
	}
	write(entry, DOI.Slot, doi)
	return entry, nil
}

func fetchArxiv(ctx context.Context, id string) (*yaml.Node, error) {
	entry, err := fetchDOI(ctx, ArxivDOI(id))
	if err != nil {
		return nil, err
	}
	write(entry, Arxiv.Slot, id)
	return entry, nil
}

// jsonText is a trimmed JSON string; anything else reads as "".
type jsonText string

func (t *jsonText) UnmarshalJSON(data []byte) error {
	var s string
	if json.Unmarshal(data, &s) == nil {
		*t = jsonText(strings.TrimSpace(s))
	}
	return nil
}

type cslItem struct {
	Title, Type, Volume, Issue, Page, DOI jsonText
	Container                             jsonText `json:"container-title"`
	Author                                []struct{ Family, Given jsonText }
	Issued                                struct {
		DateParts [][]json.RawMessage `json:"date-parts"`
	}
}

func fetchPMID(ctx context.Context, id string) (*yaml.Node, error) {
	body, err := fetchText(ctx, "https://pmc.ncbi.nlm.nih.gov/api/ctxp/v1/pubmed/?format=csl&id="+id, "*/*")
	if err != nil {
		return nil, err
	}
	var item cslItem
	if json.Unmarshal([]byte(body), &item) != nil {
		return nil, errNoRecord
	}
	if item.Title == "" {
		return nil, ErrNotFound
	}
	kind, parentKind := typeArticle, typePeriodical
	switch item.Type {
	case "book":
		kind, parentKind = typeBook, typeBook
	case "chapter":
		kind, parentKind = typeChapter, typeBook
	}
	entry := newMap()
	write(entry, Slot{Kind: SlotType}, kind)
	write(entry, Title.Slot, string(item.Title))
	var authors []string
	for _, person := range item.Author {
		if person.Family == "" {
			continue
		}
		if person.Given == "" {
			authors = append(authors, string(person.Family))
		} else {
			authors = append(authors, string(person.Family)+", "+string(person.Given))
		}
	}
	write(entry, Authors.Slot, strings.Join(authors, "; "))
	if len(item.Issued.DateParts) > 0 && len(item.Issued.DateParts[0]) > 0 {
		if year, ok := jsonYear(item.Issued.DateParts[0][0]); ok {
			write(entry, Date.Slot, strconv.FormatInt(year, 10))
		}
	}
	if item.Container != "" {
		write(entry, Slot{Kind: SlotParentType}, parentKind)
		write(entry, parentTitle, string(item.Container))
		write(entry, parentVolume, string(item.Volume))
		write(entry, parentIssue, string(item.Issue))
	}
	write(entry, Pages.Slot, string(item.Page))
	write(entry, pmidSlot, id)
	write(entry, DOI.Slot, string(item.DOI))
	return entry, nil
}

func jsonYear(raw json.RawMessage) (int64, bool) {
	var n int64
	if json.Unmarshal(raw, &n) == nil {
		return n, true
	}
	var s string
	if json.Unmarshal(raw, &s) != nil {
		return 0, false
	}
	n, err := strconv.ParseInt(s, 10, 64)
	return n, err == nil
}

// sortName is "Family, Given" from a name written in reading order.
func sortName(name string) string {
	trimmed := strings.TrimSpace(name)
	if at := strings.LastIndexByte(trimmed, ' '); at >= 0 && !strings.Contains(name, ",") {
		return trimmed[at+1:] + ", " + trimmed[:at]
	}
	return trimmed
}

type openLibraryBook struct {
	Title, Subtitle jsonText
	PublishDate     jsonText `json:"publish_date"`
	Authors         []struct{ Name jsonText }
	Publishers      []struct{ Name jsonText }
}

func fetchISBN(ctx context.Context, id string) (*yaml.Node, error) {
	key := "ISBN:" + id
	body, err := fetchText(ctx, "https://openlibrary.org/api/books?bibkeys="+key+"&format=json&jscmd=data", "application/json")
	if err != nil {
		return nil, err
	}
	var books map[string]openLibraryBook
	if json.Unmarshal([]byte(body), &books) != nil {
		return nil, errNoRecord
	}
	book, ok := books[key]
	if !ok || book.Title == "" {
		return nil, ErrNotFound
	}
	title := string(book.Title)
	if book.Subtitle != "" {
		title += ": " + string(book.Subtitle)
	}
	entry := newMap()
	write(entry, Slot{Kind: SlotType}, typeBook)
	write(entry, Title.Slot, title)
	var authors []string
	for _, author := range book.Authors {
		if author.Name != "" {
			authors = append(authors, sortName(string(author.Name)))
		}
	}
	write(entry, Authors.Slot, strings.Join(authors, "; "))
	if year, ok := firstYear(string(book.PublishDate)); ok {
		write(entry, Date.Slot, year)
	}
	if len(book.Publishers) > 0 {
		write(entry, Publisher.Slot, string(book.Publishers[0].Name))
	}
	write(entry, ISBN.Slot, id)
	return entry, nil
}

// firstYear is the first run of four digits in date.
func firstYear(date string) (string, bool) {
	for i := 0; i+4 <= len(date); i++ {
		if !allDigits(date[i : i+4]) {
			continue
		}
		if n, err := strconv.Atoi(date[i : i+4]); err == nil {
			return strconv.Itoa(n), true
		}
	}
	return "", false
}

type oaLocation struct {
	PDF jsonText `json:"url_for_pdf"`
}

// unpaywallLinks are Unpaywall's PDF links for doi, best first. Empty when it has no record.
func unpaywallLinks(ctx context.Context, doi, email string) ([]string, error) {
	body, err := fetchText(ctx, "https://api.unpaywall.org/v2/"+encodePath(doi)+"?email="+encodePath(email), "application/json")
	if errors.Is(err, ErrNotFound) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	var record struct {
		Best   *oaLocation  `json:"best_oa_location"`
		Others []oaLocation `json:"oa_locations"`
	}
	if json.Unmarshal([]byte(body), &record) != nil {
		return nil, ErrUnpaywallNoRecord
	}
	locations := record.Others
	if record.Best != nil {
		locations = append([]oaLocation{*record.Best}, locations...)
	}
	var links []string
	for _, location := range locations {
		if link := string(location.PDF); link != "" && !contains(links, link) {
			links = append(links, link)
		}
	}
	return links, nil
}

// download saves the PDF at url to dest, whole or not at all.
func download(ctx context.Context, url, dest string) (err error) {
	if !strings.HasPrefix(url, "https://") && !strings.HasPrefix(url, "http://") {
		return ErrNotWebLink
	}
	response, err := request(ctx, url, "application/pdf", paperTimeout)
	if err != nil {
		return err
	}
	defer closeBody(response.Body, &err)
	data, err := io.ReadAll(io.LimitReader(response.Body, paperLimit+1))
	if err != nil {
		return fmt.Errorf("download: %w", err)
	}
	if len(data) > paperLimit {
		return ErrPaperTooLarge
	}
	// Publishers often answer a PDF link with a login page.
	if !bytes.HasPrefix(data, []byte(vault.PDFHeader)) {
		return ErrNotPDF
	}
	if err := os.MkdirAll(filepath.Dir(dest), 0o755); err != nil {
		return fmt.Errorf("download: %w", err)
	}
	return vault.WriteAtomic(dest, data)
}

// FetchPaper downloads an open-access copy to dest: arXiv first, then Unpaywall
// when there is an email to ask with. False when neither knows a copy.
func FetchPaper(ctx context.Context, ids PaperIDs, email, dest string) (bool, error) {
	var links []string
	if id := ids.ArxivID(); id != "" {
		links = append(links, "https://arxiv.org/pdf/"+encodePath(id))
	}
	var problem error
	if ids.DOI != "" && email != "" {
		found, err := unpaywallLinks(ctx, ids.DOI, email)
		if err != nil {
			problem = err
		} else {
			links = append(links, found...)
		}
	}
	for _, link := range links {
		err := download(ctx, link, dest)
		if err == nil {
			return true, nil
		}
		problem = err
	}
	return false, problem
}

// Fetch looks up one identifier as a Hayagriva entry, without its key.
func Fetch(ctx context.Context, id Identifier) (Entry, error) {
	var fetch func(context.Context, string) (*yaml.Node, error)
	switch id.Kind {
	case IDDOI:
		fetch = fetchDOI
	case IDArxiv:
		fetch = fetchArxiv
	case IDPMID:
		fetch = fetchPMID
	case IDISBN:
		fetch = fetchISBN
	}
	if fetch == nil {
		return Entry{}, fmt.Errorf("%w %d", ErrUnknownIdentifier, id.Kind)
	}
	node, err := fetch(ctx, id.Value)
	if err != nil {
		return Entry{}, err
	}
	return Entry{node}, nil
}
