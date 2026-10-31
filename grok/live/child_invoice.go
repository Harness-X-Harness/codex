package live

import (
	"encoding/csv"
	"regexp"
	"strconv"
	"strings"
)

const childInvoiceTask = "Please ask a subagent to check this order and prepare a CSV invoice: 3 notebooks at 4.50 each and 2 pens at 1.25 each, with no tax or shipping. Wait for the subagent's result, then return the invoice. Use columns item, quantity, unit_price, line_total; label the items notebook and pen, and include a total row with empty quantity and unit_price. Return one complete CSV, either plain or in one CSV code block."

// The CSV is the requested business artifact, not a model-facing proof token.
// Compare money as integer cents; presentation order and equivalent decimals
// do not change the invoice. A second report is never silently selected away.
func validChildInvoice(text string) bool {
	text = strings.TrimSpace(strings.TrimPrefix(strings.TrimSpace(text), "\ufeff"))
	if text == "" {
		return false
	}
	if strings.Contains(text, "```") {
		lines := strings.Split(text, "\n")
		start, end := -1, -1
		for i, line := range lines {
			line = strings.TrimSpace(line)
			if !strings.Contains(line, "```") {
				continue
			}
			if start < 0 && (strings.EqualFold(line, "```csv") || line == "```") {
				start = i
			} else if start >= 0 && end < 0 && line == "```" {
				end = i
			} else {
				return false
			}
		}
		if start < 0 || end <= start || hasInvoiceHeader(strings.Join(lines[:start], "\n")+"\n"+strings.Join(lines[end+1:], "\n")) {
			return false
		}
		text = strings.Join(lines[start+1:end], "\n")
	}
	r := csv.NewReader(strings.NewReader(text))
	r.TrimLeadingSpace, r.FieldsPerRecord = true, -1
	rows, err := r.ReadAll()
	if err != nil || len(rows) != 4 || len(rows[0]) != 4 {
		return false
	}
	columns := invoiceColumns(rows[0])
	if columns == nil {
		return false
	}
	seen := make(map[string]bool)
	for _, row := range rows[1:] {
		if len(row) != 4 {
			return false
		}
		get := func(name string) string { return strings.TrimSpace(row[columns[name]]) }
		item := strings.ToLower(get("item"))
		if seen[item] {
			return false
		}
		seen[item] = true
		quantity, price, total := get("quantity"), get("unit_price"), get("line_total")
		cents, ok := invoiceCents(total)
		if !ok {
			return false
		}
		if item == "total" {
			if quantity != "" || price != "" || cents != 1600 {
				return false
			}
			continue
		}
		scaledQuantity, quantityOK := invoiceCents(quantity)
		q := scaledQuantity / 100
		p, ok := invoiceCents(price)
		if !quantityOK || scaledQuantity%100 != 0 || !ok || q*p != cents {
			return false
		}
		switch item {
		case "notebook":
			if q != 3 || p != 450 || cents != 1350 {
				return false
			}
		case "pen":
			if q != 2 || p != 125 || cents != 250 {
				return false
			}
		default:
			return false
		}
	}
	return seen["notebook"] && seen["pen"] && seen["total"]
}

func invoiceColumns(row []string) map[string]int {
	if len(row) != 4 {
		return nil
	}
	columns := make(map[string]int)
	for i, name := range row {
		name = strings.ToLower(strings.TrimSpace(strings.TrimPrefix(strings.TrimSpace(name), "\ufeff")))
		switch name {
		case "item", "quantity", "unit_price", "line_total":
		default:
			return nil
		}
		if _, duplicate := columns[name]; duplicate {
			return nil
		}
		columns[name] = i
	}
	return columns
}

// Find only possible four-column headers, then let encoding/csv and the same
// column validator decide. RE2 scans the prose once, including quoted multiline
// fields; restarting a CSV reader at every line would rescan malformed tails.
var invoiceHeader = func() *regexp.Regexp {
	space := `[\t\n\v\f\r \p{Z}\x{0085}]`
	inline := `[\t\v\f\r \p{Z}\x{0085}]`
	name := `(?:item|quantity|unit_price|line_total)`
	field := inline + `*(?:"` + space + `*\x{FEFF}?` + space + `*` + name + space + `*"|\x{FEFF}?` + inline + `*` + name + `)` + inline + `*`
	return regexp.MustCompile(`(?m)^` + inline + `*\x{FEFF}?` + field + `(?:,` + field + `){3}$`)
}()

func hasInvoiceHeader(text string) bool {
	text = strings.ToLower(text) // same case normalization as invoiceColumns
	for {
		match := invoiceHeader.FindStringIndex(text)
		if match == nil {
			return false
		}
		r := csv.NewReader(strings.NewReader(strings.TrimPrefix(strings.TrimSpace(text[match[0]:match[1]]), "\ufeff")))
		r.TrimLeadingSpace, r.FieldsPerRecord = true, -1
		row, err := r.Read()
		if err == nil && invoiceColumns(row) != nil {
			return true
		}
		text = text[match[1]:]
	}
}

func invoiceCents(text string) (int, bool) {
	whole, fraction, _ := strings.Cut(text, ".")
	if whole == "" {
		return 0, false
	}
	for _, part := range []string{whole, fraction} {
		for _, digit := range part {
			if digit < '0' || digit > '9' {
				return 0, false
			}
		}
	}
	// Representation length does not change a value. Keep significant digits
	// exact and reject fractional cents or integer overflow, never round.
	whole = strings.TrimLeft(whole, "0")
	if whole == "" {
		whole = "0"
	}
	fraction = strings.TrimRight(fraction, "0")
	if len(fraction) > 2 {
		return 0, false
	}
	for len(fraction) < 2 {
		fraction += "0"
	}
	w, err := strconv.Atoi(whole)
	f, ferr := strconv.Atoi(fraction)
	maxInt := int(^uint(0) >> 1)
	if err != nil || ferr != nil || w > (maxInt-f)/100 {
		return 0, false
	}
	return w*100 + f, true
}
