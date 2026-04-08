# RuddyDoc Document Ontology

The RuddyDoc ontology defines how parsed documents are represented as RDF knowledge graphs in an embedded Oxigraph store. It covers document hierarchy, text elements, tables, pictures, spatial layout (bounding boxes), provenance (how elements were detected), and cross-references.

## Namespace and Prefix

| Prefix     | IRI                                             |
|------------|-------------------------------------------------|
| `rdoc:`    | `https://ruddydoc.chapeaux.io/ontology#`        |
| `rdf:`     | `http://www.w3.org/1999/02/22-rdf-syntax-ns#`   |
| `rdfs:`    | `http://www.w3.org/2000/01/rdf-schema#`          |
| `xsd:`     | `http://www.w3.org/2001/XMLSchema#`              |
| `dcterms:` | `http://purl.org/dc/terms/`                     |
| `schema:`  | `https://schema.org/`                            |

**Named graph conventions:**

- Ontology graph: `urn:ruddydoc:ontology`
- Per-document graph: `urn:ruddydoc:doc:{document_hash}`
- Element IRI: `urn:ruddydoc:doc:{document_hash}/{element_id}`

Each parsed document lives in its own named graph, enabling multi-document queries while keeping documents isolated.

## Files

| File             | Purpose                                                |
|------------------|--------------------------------------------------------|
| `ruddydoc.ttl`   | The document ontology (classes, properties, mappings)  |
| `shapes.ttl`     | SHACL validation shapes for document graphs            |

## Class Hierarchy

```
rdoc:Document (subClassOf schema:CreativeWork)
rdoc:Page

rdoc:DocumentElement
  rdoc:TextElement
    rdoc:Title
    rdoc:SectionHeader
    rdoc:Paragraph
    rdoc:ListItem
    rdoc:Footnote
    rdoc:Caption
    rdoc:Code (subClassOf schema:SoftwareSourceCode)
    rdoc:Formula
    rdoc:Reference
    rdoc:Hyperlink
  rdoc:TableElement (subClassOf schema:Table)
  rdoc:PictureElement (subClassOf schema:ImageObject)
  rdoc:KeyValueItem
  rdoc:Group
    rdoc:OrderedList
    rdoc:UnorderedList
  rdoc:Furniture
    rdoc:PageHeader
    rdoc:PageFooter

rdoc:TableCell
rdoc:BoundingBox
rdoc:Provenance
```

## Schema.org Bridge

RuddyDoc bridges to schema.org for document metadata, enabling JSON-LD export that is compatible with Google Structured Data and other consumers of schema.org:

- `rdoc:Document` is a subclass of `schema:CreativeWork`
- `rdoc:PictureElement` is a subclass of `schema:ImageObject`
- `rdoc:TableElement` is a subclass of `schema:Table`
- `rdoc:Code` is a subclass of `schema:SoftwareSourceCode`
- `dcterms:title` is equivalent to `schema:name`
- `dcterms:creator` is equivalent to `schema:author`
- `dcterms:date` is equivalent to `schema:datePublished`
- `rdoc:language` is equivalent to `schema:inLanguage`
- `rdoc:pageCount` is equivalent to `schema:numberOfPages`

Document-level metadata uses Dublin Core (`dcterms:title`, `dcterms:creator`, `dcterms:date`). The schema.org equivalences are declared in the ontology so the JSON-LD exporter can produce the correct output automatically.

## Example SPARQL Queries

### List all paragraphs in reading order

```sparql
PREFIX rdoc: <https://ruddydoc.chapeaux.io/ontology#>

SELECT ?paragraph ?text ?order
WHERE {
  ?paragraph a rdoc:Paragraph ;
             rdoc:textContent ?text ;
             rdoc:readingOrder ?order .
}
ORDER BY ?order
```

### Find all tables and their cell counts

```sparql
PREFIX rdoc: <https://ruddydoc.chapeaux.io/ontology#>

SELECT ?table (COUNT(?cell) AS ?cellCount)
WHERE {
  ?table a rdoc:TableElement ;
         rdoc:hasCell ?cell .
}
GROUP BY ?table
```

### Get document structure with heading hierarchy

```sparql
PREFIX rdoc:    <https://ruddydoc.chapeaux.io/ontology#>
PREFIX dcterms: <http://purl.org/dc/terms/>

SELECT ?title ?heading ?level ?headingText ?order
WHERE {
  ?doc a rdoc:Document .
  OPTIONAL { ?doc dcterms:title ?title }
  ?heading a rdoc:SectionHeader ;
           rdoc:headingLevel ?level ;
           rdoc:textContent ?headingText ;
           rdoc:readingOrder ?order .
}
ORDER BY ?order
```

### Find pictures with captions

```sparql
PREFIX rdoc: <https://ruddydoc.chapeaux.io/ontology#>

SELECT ?picture ?format ?captionText
WHERE {
  ?picture a rdoc:PictureElement ;
           rdoc:pictureFormat ?format ;
           rdoc:hasCaption ?caption .
  ?caption rdoc:textContent ?captionText .
}
```

### Query across multiple documents (using named graphs)

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
```
