# SPARQL Cookbook

A collection of useful SPARQL queries for RuddyDoc document graphs.

## Namespace prefix

All queries use the RuddyDoc ontology namespace:

```sparql
PREFIX rdoc: <https://ruddydoc.chapeaux.io/ontology#>
PREFIX dcterms: <http://purl.org/dc/terms/>
```

## Basic queries

### List all headings in reading order

```sparql
PREFIX rdoc: <https://ruddydoc.chapeaux.io/ontology#>

SELECT ?text ?level ?order
WHERE {
  ?h a rdoc:SectionHeader ;
     rdoc:textContent ?text ;
     rdoc:headingLevel ?level ;
     rdoc:readingOrder ?order .
}
ORDER BY ?order
```

### Count elements by type

```sparql
PREFIX rdoc: <https://ruddydoc.chapeaux.io/ontology#>

SELECT ?type (COUNT(?element) AS ?count)
WHERE {
  ?element a ?type .
  FILTER(STRSTARTS(STR(?type), "https://ruddydoc.chapeaux.io/ontology#"))
}
GROUP BY ?type
ORDER BY DESC(?count)
```

### Get document metadata

```sparql
PREFIX rdoc: <https://ruddydoc.chapeaux.io/ontology#>
PREFIX dcterms: <http://purl.org/dc/terms/>

SELECT ?title ?author ?format ?pages
WHERE {
  ?doc a rdoc:Document .
  OPTIONAL { ?doc dcterms:title ?title }
  OPTIONAL { ?doc dcterms:creator ?author }
  OPTIONAL { ?doc rdoc:sourceFormat ?format }
  OPTIONAL { ?doc rdoc:pageCount ?pages }
}
```

## Text content queries

### Find all paragraphs containing a keyword

```sparql
PREFIX rdoc: <https://ruddydoc.chapeaux.io/ontology#>

SELECT ?text ?order
WHERE {
  ?p a rdoc:Paragraph ;
     rdoc:textContent ?text ;
     rdoc:readingOrder ?order .
  FILTER(CONTAINS(LCASE(?text), "machine learning"))
}
ORDER BY ?order
```

### Get text under a specific heading

```sparql
PREFIX rdoc: <https://ruddydoc.chapeaux.io/ontology#>

SELECT ?elemText ?elemOrder
WHERE {
  # Find the target heading
  ?heading a rdoc:SectionHeader ;
           rdoc:textContent ?headingText ;
           rdoc:readingOrder ?headingOrder .
  FILTER(CONTAINS(LCASE(?headingText), "introduction"))
  
  # Find the next heading at the same or higher level
  OPTIONAL {
    ?nextHeading a rdoc:SectionHeader ;
                 rdoc:headingLevel ?nextLevel ;
                 rdoc:readingOrder ?nextOrder .
    ?heading rdoc:headingLevel ?targetLevel .
    FILTER(?nextOrder > ?headingOrder && ?nextLevel <= ?targetLevel)
  }
  
  # Get all elements between this heading and the next
  ?elem rdoc:readingOrder ?elemOrder ;
        rdoc:textContent ?elemText .
  FILTER(?elemOrder > ?headingOrder && 
         (!BOUND(?nextOrder) || ?elemOrder < ?nextOrder))
}
ORDER BY ?elemOrder
```

### Extract all list items

```sparql
PREFIX rdoc: <https://ruddydoc.chapeaux.io/ontology#>

SELECT ?text ?order ?parent
WHERE {
  ?item a rdoc:ListItem ;
        rdoc:textContent ?text ;
        rdoc:readingOrder ?order .
  OPTIONAL { ?item rdoc:parentElement ?parent }
}
ORDER BY ?order
```

## Table queries

### Find all tables with more than 5 rows

```sparql
PREFIX rdoc: <https://ruddydoc.chapeaux.io/ontology#>

SELECT ?table ?rows ?cols
WHERE {
  ?table a rdoc:TableElement ;
         rdoc:rowCount ?rows ;
         rdoc:columnCount ?cols .
  FILTER(?rows > 5)
}
```

### Get table data as rows

```sparql
PREFIX rdoc: <https://ruddydoc.chapeaux.io/ontology#>

SELECT ?table ?row ?col ?text
WHERE {
  ?table a rdoc:TableElement ;
         rdoc:hasCell ?cell .
  ?cell rdoc:cellRow ?row ;
        rdoc:cellColumn ?col ;
        rdoc:cellText ?text .
}
ORDER BY ?table ?row ?col
```

### Find tables with headers

```sparql
PREFIX rdoc: <https://ruddydoc.chapeaux.io/ontology#>

SELECT ?table (COUNT(?headerCell) AS ?headerCount)
WHERE {
  ?table a rdoc:TableElement ;
         rdoc:hasCell ?headerCell .
  ?headerCell rdoc:isHeader true .
}
GROUP BY ?table
HAVING (COUNT(?headerCell) > 0)
```

### Extract first row of all tables

```sparql
PREFIX rdoc: <https://ruddydoc.chapeaux.io/ontology#>

SELECT ?table ?col ?text
WHERE {
  ?table a rdoc:TableElement ;
         rdoc:hasCell ?cell .
  ?cell rdoc:cellRow 0 ;
        rdoc:cellColumn ?col ;
        rdoc:cellText ?text .
}
ORDER BY ?table ?col
```

## Picture queries

### Find all pictures with captions

```sparql
PREFIX rdoc: <https://ruddydoc.chapeaux.io/ontology#>

SELECT ?picture ?format ?captionText ?order
WHERE {
  ?picture a rdoc:PictureElement ;
           rdoc:pictureFormat ?format ;
           rdoc:hasCaption ?caption ;
           rdoc:readingOrder ?order .
  ?caption rdoc:textContent ?captionText .
}
ORDER BY ?order
```

### Count pictures by format

```sparql
PREFIX rdoc: <https://ruddydoc.chapeaux.io/ontology#>

SELECT ?format (COUNT(?picture) AS ?count)
WHERE {
  ?picture a rdoc:PictureElement ;
           rdoc:pictureFormat ?format .
}
GROUP BY ?format
ORDER BY DESC(?count)
```

### Find pictures with alt text

```sparql
PREFIX rdoc: <https://ruddydoc.chapeaux.io/ontology#>

SELECT ?picture ?altText ?format
WHERE {
  ?picture a rdoc:PictureElement ;
           rdoc:altText ?altText ;
           rdoc:pictureFormat ?format .
}
```

## Spatial layout queries

### Find elements on a specific page

```sparql
PREFIX rdoc: <https://ruddydoc.chapeaux.io/ontology#>

SELECT ?element ?type ?text ?page
WHERE {
  ?element rdoc:onPage ?page ;
           a ?type .
  ?page rdoc:pageNumber 5 .
  OPTIONAL { ?element rdoc:textContent ?text }
}
```

### Get elements with bounding boxes

```sparql
PREFIX rdoc: <https://ruddydoc.chapeaux.io/ontology#>

SELECT ?element ?left ?top ?right ?bottom
WHERE {
  ?element rdoc:hasBoundingBox ?bbox .
  ?bbox rdoc:bboxLeft ?left ;
        rdoc:bboxTop ?top ;
        rdoc:bboxRight ?right ;
        rdoc:bboxBottom ?bottom .
}
```

### Find elements in a spatial region

```sparql
PREFIX rdoc: <https://ruddydoc.chapeaux.io/ontology#>

SELECT ?element ?type
WHERE {
  ?element rdoc:hasBoundingBox ?bbox ;
           a ?type .
  ?bbox rdoc:bboxLeft ?left ;
        rdoc:bboxTop ?top ;
        rdoc:bboxRight ?right ;
        rdoc:bboxBottom ?bottom .
  # Top-left quadrant of the page
  FILTER(?left < 306 && ?top < 396)
}
```

## Document structure queries

### Build heading hierarchy

```sparql
PREFIX rdoc: <https://ruddydoc.chapeaux.io/ontology#>

SELECT ?heading ?level ?text ?parent ?order
WHERE {
  ?heading a rdoc:SectionHeader ;
           rdoc:headingLevel ?level ;
           rdoc:textContent ?text ;
           rdoc:readingOrder ?order .
  OPTIONAL { ?heading rdoc:parentElement ?parent }
}
ORDER BY ?order
```

### Find all top-level sections

```sparql
PREFIX rdoc: <https://ruddydoc.chapeaux.io/ontology#>

SELECT ?heading ?text
WHERE {
  ?heading a rdoc:SectionHeader ;
           rdoc:headingLevel 1 ;
           rdoc:textContent ?text .
}
```

### Get document tree depth

```sparql
PREFIX rdoc: <https://ruddydoc.chapeaux.io/ontology#>

SELECT (MAX(?level) AS ?maxDepth)
WHERE {
  ?heading a rdoc:SectionHeader ;
           rdoc:headingLevel ?level .
}
```

## Cross-document queries

Queries across multiple documents use named graphs.

### List all documents in the store

```sparql
PREFIX rdoc: <https://ruddydoc.chapeaux.io/ontology#>

SELECT ?graph ?format ?pages ?hash
WHERE {
  GRAPH ?graph {
    ?doc a rdoc:Document ;
         rdoc:sourceFormat ?format ;
         rdoc:documentHash ?hash .
    OPTIONAL { ?doc rdoc:pageCount ?pages }
  }
}
```

### Find documents containing a keyword

```sparql
PREFIX rdoc: <https://ruddydoc.chapeaux.io/ontology#>

SELECT DISTINCT ?graph ?format
WHERE {
  GRAPH ?graph {
    ?doc a rdoc:Document ;
         rdoc:sourceFormat ?format .
    ?elem rdoc:textContent ?text .
    FILTER(CONTAINS(LCASE(?text), "ruddydoc"))
  }
}
```

### Compare element counts across documents

```sparql
PREFIX rdoc: <https://ruddydoc.chapeaux.io/ontology#>

SELECT ?graph ?format (COUNT(?element) AS ?elementCount)
WHERE {
  GRAPH ?graph {
    ?doc a rdoc:Document ;
         rdoc:sourceFormat ?format ;
         rdoc:hasElement ?element .
  }
}
GROUP BY ?graph ?format
ORDER BY DESC(?elementCount)
```

## Advanced queries

### Find code blocks by language

```sparql
PREFIX rdoc: <https://ruddydoc.chapeaux.io/ontology#>

SELECT ?code ?lang ?text
WHERE {
  ?code a rdoc:Code ;
        rdoc:codeLanguage ?lang ;
        rdoc:textContent ?text .
  FILTER(?lang = "python")
}
```

### Extract bibliographic references

```sparql
PREFIX rdoc: <https://ruddydoc.chapeaux.io/ontology#>

SELECT ?ref ?key ?text ?order
WHERE {
  ?ref a rdoc:Reference ;
       rdoc:textContent ?text ;
       rdoc:readingOrder ?order .
  OPTIONAL { ?ref rdoc:citationKey ?key }
}
ORDER BY ?order
```

### Find elements with low ML confidence

```sparql
PREFIX rdoc: <https://ruddydoc.chapeaux.io/ontology#>

SELECT ?element ?type ?confidence ?detectedBy
WHERE {
  ?element a ?type ;
           rdoc:confidence ?confidence ;
           rdoc:detectedBy ?detectedBy .
  FILTER(?confidence < 0.7)
}
ORDER BY ?confidence
```

### Get formulas in LaTeX notation

```sparql
PREFIX rdoc: <https://ruddydoc.chapeaux.io/ontology#>

SELECT ?formula ?text ?order
WHERE {
  ?formula a rdoc:Formula ;
           rdoc:textContent ?text ;
           rdoc:readingOrder ?order .
}
ORDER BY ?order
```

## Usage tips

### Running queries from the CLI

```bash
# Use the query command
ruddydoc query 'SELECT ?h ?t WHERE {
  ?h a <https://ruddydoc.chapeaux.io/ontology#SectionHeader> ;
     rdoc:textContent ?t .
}' document.pdf

# Output as JSON
ruddydoc query '...' document.pdf --format json

# Query multiple documents
ruddydoc query '...' doc1.pdf doc2.md doc3.html
```

### Query formatting

For readability, use multi-line queries:

```bash
ruddydoc query '
  PREFIX rdoc: <https://ruddydoc.chapeaux.io/ontology#>
  
  SELECT ?text ?level
  WHERE {
    ?h a rdoc:SectionHeader ;
       rdoc:textContent ?text ;
       rdoc:headingLevel ?level .
  }
  ORDER BY ?level
' document.pdf
```

### Performance considerations

1. **Always use ORDER BY** for deterministic results
2. **Use OPTIONAL** sparingly — it can be slow on large graphs
3. **Filter early** — apply FILTER clauses close to the triple patterns they constrain
4. **Limit results** for exploration — add `LIMIT 10` during development
5. **Use named graphs explicitly** for multi-document queries to avoid cross-contamination
