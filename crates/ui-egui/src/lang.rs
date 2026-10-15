//! The interface's language: English, as the code writes it, or Brazilian Portuguese. What the
//! window shows goes through [`t`], English being its own key; the widget helpers translate their
//! labels and tips, so most call sites pass English and read it back translated. What a project
//! holds (names, what its drawings say) follows the project's language instead, and the core's
//! messages, which agents read too, stay English.

use std::cell::Cell;

use bimcraft_core::language::{self, Language};

thread_local! {
    /// The language this frame is drawn in: each window draws on its own thread.
    static LANG: Cell<Language> = const { Cell::new(Language::English) };
}

pub fn set(language: Language) {
    LANG.with(|l| l.set(language));
}

pub fn get() -> Language {
    LANG.with(Cell::get)
}

/// The language the environment asks for (`LC_ALL`, `LC_MESSAGES`, `LANG`): Portuguese for any
/// `pt…`, else English.
pub fn from_env() -> Language {
    let asked = ["LC_ALL", "LC_MESSAGES", "LANG"].iter().filter_map(|k| std::env::var(k).ok()).find(|v| !v.is_empty());
    if asked.is_some_and(|v| v.to_ascii_lowercase().starts_with("pt")) { Language::Portuguese } else { Language::English }
}

/// `english` in the interface's language.
pub fn t(english: &str) -> &str {
    match get() {
        Language::English => english,
        Language::Portuguese => portuguese(english).unwrap_or(english),
    }
}

/// `template` translated, its `{}`s filled with `args` in order.
pub fn tf(template: &str, args: &[&str]) -> String {
    args.iter().fold(t(template).to_string(), |s, a| s.replacen("{}", a, 1))
}

/// An element category's name, of one (`Wall`) or of several (`Walls`).
pub fn category(category: &str, several: bool) -> String {
    let names = match category {
        "wall" => ("Wall", "Walls", "Parede", "Paredes"),
        "door" => ("Door", "Doors", "Porta", "Portas"),
        "window" => ("Window", "Windows", "Janela", "Janelas"),
        "floor" => ("Floor", "Floors", "Piso", "Pisos"),
        "room" => ("Room", "Rooms", "Ambiente", "Ambientes"),
        "level" => ("Level", "Levels", "Nível", "Níveis"),
        "grid" => ("Grid", "Grids", "Eixo", "Eixos"),
        "dimension" => ("Dimension", "Dimensions", "Cota", "Cotas"),
        "tag" => ("Tag", "Tags", "Identificador", "Identificadores"),
        "section" => ("Section", "Sections", "Corte", "Cortes"),
        "elevation" => ("Elevation", "Elevations", "Elevação", "Elevações"),
        "sheet" => ("Sheet", "Sheets", "Folha", "Folhas"),
        "viewport" => ("Viewport", "Viewports", "Janela de vista", "Janelas de vista"),
        "column" => ("Column", "Columns", "Pilar", "Pilares"),
        "stair" => ("Stair", "Stairs", "Escada", "Escadas"),
        "roof" => ("Roof", "Roofs", "Telhado", "Telhados"),
        "text" => ("Text Note", "Text Notes", "Nota de texto", "Notas de texto"),
        "spot" => ("Spot Elevation", "Spot Elevations", "Elevação de ponto", "Elevações de ponto"),
        "schedule" => ("Schedule", "Schedules", "Tabela", "Tabelas"),
        "view3d" => ("3D View", "3D Views", "Vista 3D", "Vistas 3D"),
        other => return other.to_string(),
    };
    let (one, many, um, varios) = names;
    match (get(), several) {
        (Language::English, false) => one,
        (Language::English, true) => many,
        (Language::Portuguese, false) => um,
        (Language::Portuguese, true) => varios,
    }
    .to_string()
}

/// The interface in Brazilian Portuguese; the core's words (schedule names and headings) besides.
pub(crate) fn portuguese(english: &str) -> Option<&'static str> {
    Some(match english {
        // The ribbon's tabs and the File menu.
        "File" => "Arquivo",
        "Architecture" => "Arquitetura",
        "Annotate" => "Anotar",
        "View" => "Vista",
        "Manage" => "Gerenciar",
        "Modify" => "Modificar",
        "New" => "Novo",
        "Open…" => "Abrir…",
        "Save" => "Salvar",
        "Save As…" => "Salvar como…",
        "Export" => "Exportar",
        "IFC…" => "IFC…",
        "View as PNG…" => "Vista como PNG…",
        "Plan as SVG…" => "Planta como SVG…",
        "Sheets as PDF…" => "Folhas como PDF…",
        "Exit" => "Sair",
        "BIMCraft project" => "Projeto BIMCraft",
        "BIMCraft project or IFC" => "Projeto BIMCraft ou IFC",
        "PNG image" => "Imagem PNG",
        "SVG drawing" => "Desenho SVG",
        // The Quick Access Toolbar and the navigation bar.
        "Open" => "Abrir",
        "Undo" => "Desfazer",
        "Redo" => "Refazer",
        "Default 3D View" => "Vista 3D padrão",
        "Zoom to Fit" => "Zoom para ajustar",
        "Zoom In" => "Aproximar",
        "Zoom Out" => "Afastar",
        "Home View" => "Vista inicial",
        // Panels.
        "Select" => "Selecionar",
        "Build" => "Construir",
        "Circulation" => "Circulação",
        "Room & Area" => "Ambiente e área",
        "Datum" => "Referência",
        "Dimension" => "Cota",
        "Text" => "Texto",
        "Tag" => "Identificar",
        "Graphics" => "Gráficos",
        "Create" => "Criar",
        "Sheet Composition" => "Composição de folha",
        "Windows" => "Janelas",
        "Settings" => "Configurações",
        "Properties" => "Propriedades",
        // Tools and their tips.
        "Modify (MD): select elements" => "Modificar (MD): selecionar elementos",
        "Wall (WA): click points; Enter finishes, Esc cancels" => "Parede (WA): clique nos pontos; Enter termina, Esc cancela",
        "Door" => "Porta",
        "Door (DR): click a wall" => "Porta (DR): clique numa parede",
        "Window" => "Janela",
        "Window (WN): click a wall" => "Janela (WN): clique numa parede",
        "Column" => "Pilar",
        "Column (CL): click where it stands" => "Pilar (CL): clique onde ele fica",
        "Roof" => "Telhado",
        "Roof by Footprint: click inside the walls to roof them, on the level above" => {
            "Telhado por perímetro: clique dentro das paredes para cobri-las, no nível acima"
        }
        "Stair" => "Escada",
        "Stair: click where it starts, then which way it climbs" => "Escada: clique onde ela começa e depois na direção em que sobe",
        "Room (RM): click inside walls" => "Ambiente (RM): clique dentro das paredes",
        "Level (LL): a new level at an elevation" => "Nível (LL): um novo nível numa elevação",
        "Grid" => "Eixo",
        "Grid (GR): click its two ends" => "Eixo (GR): clique nas duas extremidades",
        "Aligned" => "Alinhada",
        "Aligned Dimension (DI): pick parallel wall faces, wall ends, doors, windows or grids, then click where the dimension line goes" => {
            "Cota alinhada (DI): selecione faces e extremidades de paredes, portas, janelas ou eixos paralelos e clique onde vai a linha de cota"
        }
        "Angular" => "Angular",
        "Angular Dimension: pick two lines that meet, then click where the arc goes" => {
            "Cota angular: selecione duas linhas que se encontram e clique onde vai o arco"
        }
        "Radial" => "Radial",
        "Radial Dimension: pick a round column, then click where the radius goes" => {
            "Cota radial: selecione um pilar redondo e clique onde vai o raio"
        }
        "Spot Elevation" => "Elevação de ponto",
        "Spot Elevation (EL): click a floor, a roof or a stair; in a section, a point" => {
            "Elevação de ponto (EL): clique num piso, num telhado ou numa escada; num corte, num ponto"
        }
        "Text (TX): type the note in the Options Bar, then click where it goes" => {
            "Texto (TX): digite a nota na Barra de opções e clique onde ela vai"
        }
        "Tag by Category" => "Identificar por categoria",
        "Tag by Category (TG): click a door or window to tag it with its mark" => {
            "Identificar por categoria (TG): clique numa porta ou janela para identificá-la com sua marca"
        }
        "Tag All" => "Identificar todos",
        "Tag every door and window in this view that has no tag" => "Identificar as portas e janelas desta vista que não têm identificador",
        "Thin Lines" => "Linhas finas",
        "Thin Lines (TL): every line one pixel wide" => "Linhas finas (TL): toda linha com um pixel de largura",
        "Dark Mode" => "Modo escuro",
        "Dark Mode: the window and the views light on dark; sheets stay paper. It starts as your system's theme" => {
            "Modo escuro: a janela e as vistas claras sobre fundo escuro; as folhas continuam papel. Começa no tema do sistema"
        }
        "Crop View" => "Recortar vista",
        "Crop View: show only the crop region; drag its edges by their grips" => {
            "Recortar vista: mostrar só a região de recorte; arraste suas bordas pelas alças"
        }
        "3D View" => "Vista 3D",
        "Open the default 3D view" => "Abrir a vista 3D padrão",
        "Section: click its start, then its end; it looks to the left of the line" => {
            "Corte: clique no início e depois no fim; ele olha para a esquerda da linha"
        }
        "Elevation: click beside the building, on the side to look at" => "Elevação: clique ao lado da construção, no lado a olhar",
        "Schedules" => "Tabelas",
        "Open a schedule" => "Abrir uma tabela",
        "Sheet" => "Folha",
        "A new sheet, with a title block" => "Uma nova folha, com carimbo",
        "Place View" => "Inserir vista",
        "Put a view on this sheet: one not on a sheet yet" => "Colocar nesta folha uma vista que ainda não está em nenhuma",
        "Every view is on a sheet." => "Todas as vistas estão em folhas.",
        "Close Inactive" => "Fechar inativas",
        "Close every view but this one" => "Fechar todas as vistas menos esta",
        "User Interface" => "Interface do usuário",
        "Show or hide the palettes, and pick the interface's language" => "Mostrar ou ocultar as paletas, e escolher o idioma da interface",
        "Project Browser" => "Navegador de projeto",
        "Language" => "Idioma",
        "Project Units" => "Unidades do projeto",
        "Project Units (UN): how lengths show and read; now {}" => {
            "Unidades do projeto (UN): como os comprimentos aparecem e são lidos; agora {}"
        }
        "Project Language" => "Idioma do projeto",
        "Project Language: what the drawings say and how new things are named; now {}" => {
            "Idioma do projeto: o que os desenhos dizem e como as coisas novas são nomeadas; agora {}"
        }
        "Millimetres" => "Milímetros",
        "Centimetres" => "Centímetros",
        "Metres" => "Metros",
        "Inches" => "Polegadas",
        "Feet" => "Pés",
        "Feet and inches" => "Pés e polegadas",
        "Show or hide the Properties palette" => "Mostrar ou ocultar a paleta Propriedades",
        "Type Properties" => "Propriedades de tipo",
        "Edit the type" => "Editar o tipo",
        "Align" => "Alinhar",
        "Align (AL): pick a reference line, then what to line up with it" => {
            "Alinhar (AL): selecione uma linha de referência e depois o que alinhar com ela"
        }
        "Offset" => "Deslocamento",
        "Offset (OF): pick a wall; a copy goes to the cursor's side" => {
            "Deslocamento (OF): selecione uma parede; uma cópia vai para o lado do cursor"
        }
        "Mirror Pick Axis" => "Espelhar - selecionar eixo",
        "Mirror, Pick Axis (MM): pick a wall or grid to mirror across" => {
            "Espelhar, selecionar eixo (MM): selecione uma parede ou um eixo como espelho"
        }
        "Mirror Draw Axis" => "Espelhar - desenhar eixo",
        "Mirror, Draw Axis (DM): click the axis's two ends" => "Espelhar, desenhar eixo (DM): clique nas duas extremidades do eixo",
        "Split Element" => "Dividir elemento",
        "Split Element (SL): click a wall where to split it" => "Dividir elemento (SL): clique numa parede onde dividi-la",
        "Move" => "Mover",
        "Move (MV): click a base point, then where it goes" => "Mover (MV): clique num ponto base e depois para onde ele vai",
        "Copy" => "Copiar",
        "Copy (CO): click a base point, then where the copy goes" => "Copiar (CO): clique num ponto base e depois onde vai a cópia",
        "Rotate" => "Rotacionar",
        "Rotate (RO): click the angle's start, then its end" => "Rotacionar (RO): clique no início do ângulo e depois no fim",
        "Trim/Extend to Corner" => "Aparar/estender para canto",
        "Trim/Extend to Corner (TR): pick two walls, on the parts to keep" => {
            "Aparar/estender para canto (TR): selecione duas paredes, nas partes a manter"
        }
        "Delete" => "Excluir",
        "Delete (DE) the selection" => "Excluir (DE) a seleção",
        // The contextual tab.
        "Place Wall" => "Inserir parede",
        "Place Door" => "Inserir porta",
        "Place Window" => "Inserir janela",
        "Place Room" => "Inserir ambiente",
        "Place Level" => "Inserir nível",
        "Place Grid" => "Inserir eixo",
        "Place Dimensions" => "Inserir cotas",
        "Place Text" => "Inserir texto",
        "Place Column" => "Inserir pilar",
        "Create Stair" => "Criar escada",
        "Create Roof Footprint" => "Criar perímetro do telhado",
        "Multi-Select" => "Seleção múltipla",
        // The Options Bar.
        "Height:" => "Altura:",
        "Unconnected" => "Desconectada",
        "Chain" => "Cadeia",
        "Width:" => "Largura:",
        "Slope:" => "Inclinação:",
        "Overhang:" => "Beiral:",
        "Gable" => "Empena",
        "Constrain" => "Restringir",
        "Multiple" => "Múltiplo",
        "Leader" => "Linha de chamada",
        "Text:" => "Texto:",
        "what the note says" => "o que a nota diz",
        "Offset:" => "Deslocamento:",
        "Angle:" => "Ângulo:",
        "Place Centre" => "Posicionar centro",
        "the next free name" => "o próximo nome livre",
        // The status bar's prompts.
        "Click to select. Drag to orbit (or Shift and the middle button), the middle button pans, the wheel zooms." => {
            "Clique para selecionar. Arraste para orbitar (ou Shift e o botão do meio); o botão do meio move a vista, a roda aproxima."
        }
        "Click a view on the sheet to select it, drag it to move it, double-click it to open it." => {
            "Clique numa vista da folha para selecioná-la, arraste para movê-la, clique duas vezes para abri-la."
        }
        "Click to select what the view cuts or sees; the middle button pans, the wheel zooms." => {
            "Clique para selecionar o que a vista corta ou vê; o botão do meio move a vista, a roda aproxima."
        }
        "Type the new level's elevation in the Options Bar, then Create." => {
            "Digite a elevação do novo nível na Barra de opções e clique em Criar."
        }
        "Type the note in the Options Bar, then click where it goes." => "Digite a nota na Barra de opções e clique onde ela vai.",
        "Click a point to mark its height." => "Clique num ponto para marcar sua altura.",
        "This tool works in plan views: open one from the Project Browser." => {
            "Esta ferramenta funciona em plantas: abra uma pelo Navegador de projeto."
        }
        "Click to select, Tab for what's underneath, Ctrl adds, Shift takes away; drag to box or to move." => {
            "Clique para selecionar, Tab para o que está embaixo, Ctrl adiciona, Shift remove; arraste para uma caixa ou para mover."
        }
        "Click to enter the wall's start point." => "Clique para indicar o ponto inicial da parede.",
        "Click to enter the next point. Enter finishes, Esc cancels, Shift keeps it square." => {
            "Clique para indicar o próximo ponto. Enter termina, Esc cancela, Shift mantém no esquadro."
        }
        "Click a wall to place the door." => "Clique numa parede para inserir a porta.",
        "Click a wall to place the window." => "Clique numa parede para inserir a janela.",
        "Click inside walls to place a room; the outline shows the room you'd get." => {
            "Clique dentro das paredes para inserir um ambiente; o contorno mostra o ambiente resultante."
        }
        "Click to enter the grid's start point." => "Clique para indicar o ponto inicial do eixo.",
        "Click to enter the grid's end point." => "Clique para indicar o ponto final do eixo.",
        "Click a base point to measure from." => "Clique num ponto base de referência.",
        "Click where the base point goes, or type a distance and press Enter. Esc cancels." => {
            "Clique onde vai o ponto base, ou digite uma distância e pressione Enter. Esc cancela."
        }
        "Click where the copy's base point goes, or type a distance and press Enter. Esc cancels." => {
            "Clique onde vai o ponto base da cópia, ou digite uma distância e pressione Enter. Esc cancela."
        }
        "Click the centre of rotation." => "Clique no centro de rotação.",
        "Click the start of the angle, or type an angle in the Options Bar." => {
            "Clique no início do ângulo, ou digite um ângulo na Barra de opções."
        }
        "Click the end of the angle. Esc cancels." => "Clique no fim do ângulo. Esc cancela.",
        "Pick a wall or grid to mirror across." => "Selecione uma parede ou um eixo como espelho.",
        "Click the start of the mirror axis." => "Clique no início do eixo de espelhamento.",
        "Click the end of the mirror axis." => "Clique no fim do eixo de espelhamento.",
        "Pick a reference line: a grid, or a wall's face or centre line." => {
            "Selecione uma linha de referência: um eixo, ou a face ou a linha de centro de uma parede."
        }
        "Pick the wall or grid to line up with the reference." => "Selecione a parede ou o eixo a alinhar com a referência.",
        "Pick the first wall, on the part to keep." => "Selecione a primeira parede, na parte a manter.",
        "Pick the second wall, on the part to keep." => "Selecione a segunda parede, na parte a manter.",
        "Click a wall where to split it." => "Clique numa parede onde dividi-la.",
        "Point to a wall: the offset goes to the cursor's side. Click to place it." => {
            "Aponte para uma parede: o deslocamento vai para o lado do cursor. Clique para inseri-lo."
        }
        "Pick a wall face, centre line or end, a door, a window or a grid to measure from." => {
            "Selecione a face, a linha de centro ou a extremidade de uma parede, uma porta, uma janela ou um eixo para medir."
        }
        "Pick the next reference, parallel to the first." => "Selecione a próxima referência, paralela à primeira.",
        "Pick more references, or click away from them where the dimension line goes. Esc cancels." => {
            "Selecione mais referências, ou clique longe delas onde vai a linha de cota. Esc cancela."
        }
        "Click a door or window to tag it with its mark." => "Clique numa porta ou janela para identificá-la com sua marca.",
        "Pick a wall, a grid or another line; then one that meets it." => {
            "Selecione uma parede, um eixo ou outra linha; depois uma que a encontre."
        }
        "Click where the arc goes: the angle on that side. Esc cancels." => "Clique onde vai o arco: o ângulo desse lado. Esc cancela.",
        "Pick a round column." => "Selecione um pilar redondo.",
        "Click where the radius goes. Esc cancels." => "Clique onde vai o raio. Esc cancela.",
        "Type the note in the Options Bar, then click where it goes: its top left corner." => {
            "Digite a nota na Barra de opções e clique onde ela vai: seu canto superior esquerdo."
        }
        "Click a floor, a roof, a stair or a wall: the spot elevation reads the height of its top there." => {
            "Clique num piso, num telhado, numa escada ou numa parede: a elevação de ponto lê a altura do topo ali."
        }
        "Click the section's start: it looks to the left of the line you draw." => {
            "Clique no início do corte: ele olha para a esquerda da linha desenhada."
        }
        "Click the section's end." => "Clique no fim do corte.",
        "Click beside the building: the elevation looks at the side you click by." => {
            "Clique ao lado da construção: a elevação olha para o lado onde você clicar."
        }
        "Click to place a column, as high as the Options Bar says." => {
            "Clique para inserir um pilar, da altura que a Barra de opções indica."
        }
        "Click where the stair starts: the middle of its first riser." => "Clique onde a escada começa: o meio do primeiro espelho.",
        "Click which way it climbs: its risers take it up to the level above. Esc cancels." => {
            "Clique na direção em que ela sobe: os espelhos a levam ao nível acima. Esc cancela."
        }
        "Click inside the walls to roof them, on the level above: the overhang runs past their faces." => {
            "Clique dentro das paredes para cobri-las, no nível acima: o beiral passa das faces."
        }
        // Messages the window writes itself.
        "This project has no levels. Add one with the Level tool." => "Este projeto não tem níveis. Adicione um com a ferramenta Nível.",
        "select what to move, copy, rotate or mirror first" => "selecione primeiro o que mover, copiar, rotacionar ou espelhar",
        "`{}` is not a height" => "`{}` não é uma altura",
        "rotate: `{}` is not an angle in degrees" => "rotacionar: `{}` não é um ângulo em graus",
        "level: `{}` is not an elevation" => "nível: `{}` não é uma elevação",
        "column: {}" => "pilar: {}",
        "wall: {}" => "parede: {}",
        "stair: `{}` is not a width" => "escada: `{}` não é uma largura",
        "roof: the slope `{}` and the overhang `{}` must be numbers" => "telhado: a inclinação `{}` e o beiral `{}` precisam ser números",
        "text: type what the note says in the Options Bar first" => "texto: digite primeiro na Barra de opções o que a nota diz",
        "align: the lines aren't parallel" => "alinhar: as linhas não são paralelas",
        "offset: `{}` is not a distance" => "deslocamento: `{}` não é uma distância",
        "`{}` is not a length" => "`{}` não é um comprimento",
        "this tool works in plan views: open one from the Project Browser" => {
            "esta ferramenta funciona em plantas: abra uma pelo Navegador de projeto"
        }
        "`{}` is not a number" => "`{}` não é um número",
        "`{}` is not a point; type x, y" => "`{}` não é um ponto; digite x, y",
        "`{}` is not a point; type x, y in mm" => "`{}` não é um ponto; digite x, y em mm",
        "{}: `{}` is not a length" => "{}: `{}` não é um comprimento",
        "Imported {}: {}." => "Importado {}: {}.",
        " Left out: {}{}." => " Deixado de fora: {}{}.",
        "; and {} more" => "; e mais {}",
        " Moved {}, {} mm to bring it near the origin." => " Movido {}, {} mm para trazê-lo perto da origem.",
        "Not enclosed: the walls around it don't close." => "Não delimitado: as paredes em volta não se fecham.",
        "Temporary dimension {}" => "Cota temporária {}",
        // The status bar, the warning box, the unsaved-changes dialog.
        "Unsaved changes" => "Alterações não salvas",
        "Error" => "Erro",
        "Note" => "Nota",
        "Dismiss" => "Dispensar",
        "Save changes to {}?" => "Salvar as alterações em {}?",
        "If you don't, the changes since it was last saved are lost." => {
            "Se não salvar, as alterações desde a última vez em que foi salvo serão perdidas."
        }
        "Don't Save" => "Não salvar",
        "Cancel" => "Cancelar",
        // Views: their kinds, the Project Browser, the view tabs, Place View.
        "Floor Plan" => "Planta de piso",
        "Schedule" => "Tabela",
        "{}: {}" => "{}: {}",
        "Floor Plan: {}" => "Planta de piso: {}",
        "Section: {}" => "Corte: {}",
        "Elevation: {}" => "Elevação: {}",
        "3D View: {3D}" => "Vista 3D: {3D}",
        "Schedule: {}" => "Tabela: {}",
        "Project Browser · {}" => "Navegador de projeto · {}",
        "Views (all)" => "Vistas (todas)",
        "Floor Plans" => "Plantas de piso",
        "3D Views" => "Vistas 3D",
        "Elevations (Building Elevation)" => "Elevações (elevação da construção)",
        "Sections (Building Section)" => "Cortes (corte da construção)",
        "Schedules/Quantities (all)" => "Tabelas/quantitativos (todas)",
        "Sheets (all)" => "Folhas (todas)",
        "Close {}" => "Fechar {}",
        "No rows yet: the {} is empty." => "Ainda sem linhas: a {} está vazia.",
        // Properties and Type Properties.
        "Edit Type" => "Editar tipo",
        "Apply" => "Aplicar",
        "Type Selector" => "Seletor de tipo",
        "Multiple Categories" => "Várias categorias",
        "Multiple Categories ({})" => "Várias categorias ({})",
        "New {}" => "Inserir: {}",
        "{} ({})" => "{} ({})",
        "Constraints" => "Restrições",
        "Dimensions" => "Dimensões",
        "Identity Data" => "Dados de identidade",
        "Other" => "Outros",
        "Extents" => "Extensões",
        "View Range" => "Intervalo da vista",
        "Camera" => "Câmera",
        "Base Constraint" => "Restrição da base",
        "Base Offset" => "Deslocamento da base",
        "Top Constraint" => "Restrição superior",
        "Unconnected Height" => "Altura desconectada",
        "Start" => "Início",
        "End" => "Fim",
        "Host Wall" => "Parede hospedeira",
        "Flip Facing" => "Inverter face",
        "Flip Hand" => "Inverter lado",
        "Sill Height" => "Altura do peitoril",
        "Area" => "Área",
        "Perimeter" => "Perímetro",
        "Location" => "Localização",
        "Value" => "Valor",
        "Kind" => "Tipo de cota",
        "Line Through" => "Linha por",
        "Text Size (mm)" => "Tamanho do texto (mm)",
        "Rotation (°)" => "Rotação (°)",
        "Host" => "Hospedeiro",
        "View Name" => "Nome da vista",
        "Category" => "Categoria",
        "Azimuth (°)" => "Azimute (°)",
        "Elevation (°)" => "Elevação (°)",
        "Leader Line" => "Linha de chamada",
        "Tagged Element" => "Elemento identificado",
        "Sheet Number" => "Número da folha",
        "Sheet Name" => "Nome da folha",
        "Size" => "Tamanho",
        "View Scale (1 : n)" => "Escala da vista (1 : n)",
        "Base Level" => "Nível da base",
        "Top Level" => "Nível superior",
        "Actual Run Width" => "Largura real do lance",
        "Maximum Riser Height" => "Altura máxima do espelho",
        "Actual Tread Depth" => "Profundidade real do piso",
        "Actual Number of Risers" => "Número real de espelhos",
        "Actual Riser Height" => "Altura real do espelho",
        "Slope (°)" => "Inclinação (°)",
        "Max Ridge Height" => "Altura máxima da cumeeira",
        "Gable Ends" => "Empenas",
        "Far Clip Offset" => "Deslocamento do recorte distante",
        "Associated Level" => "Nível associado",
        "Level Elevation" => "Elevação do nível",
        "Top" => "Superior",
        "Cut Plane" => "Plano de corte",
        "Bottom" => "Inferior",
        "View Depth" => "Profundidade da vista",
        "Level Above" => "Nível acima",
        "Default Thickness" => "Espessura padrão",
        "Default Sill Height" => "Altura padrão do peitoril",
        "Depth" => "Profundidade",
        "Yes" => "Sim",
        "No" => "Não",
        "Family:" => "Família:",
        "Type:" => "Tipo:",
        "Duplicate…" => "Duplicar…",
        "New type" => "Novo tipo",
        "Type Parameters" => "Parâmetros de tipo",
        "Parameter" => "Parâmetro",
        "OK" => "OK",
        other => return language::portuguese(other),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_is_its_own_key_and_portuguese_falls_back_to_the_cores_words() {
        set(Language::English);
        assert_eq!((t("Wall Schedule"), t("Wall")), ("Wall Schedule", "Wall"));
        set(Language::Portuguese);
        assert_eq!((t("Wall Schedule"), t("Arquitetura"), t("Architecture")), ("Tabela de paredes", "Arquitetura", "Arquitetura"));
        assert_eq!(
            tf("roof: the slope `{}` and the overhang `{}` must be numbers", &["x", "y"]),
            "telhado: a inclinação `x` e o beiral `y` precisam ser números"
        );
        assert_eq!((category("wall", true), category("spot", false)), ("Paredes".to_string(), "Elevação de ponto".to_string()));
        set(Language::English);
    }
}
