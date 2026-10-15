//! The interface's language: English, as the code writes it, Brazilian Portuguese or Spanish. What
//! the window shows goes through [`t`], English being its own key; the widget helpers translate
//! their labels and tips, so most call sites pass English and read it back translated. What a
//! project holds (names, what its drawings say) follows the project's language instead, and the
//! core's messages, which agents read too, stay English.

use std::cell::Cell;

use bimcraft_core::language::Language;

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

/// The language the system's settings ask for (on Linux, `LC_ALL`, `LC_MESSAGES` or `LANG`), when
/// it's one there is; else English.
pub fn from_env() -> Language {
    sys_locale::get_locale().and_then(|l| Language::from_code(&l)).unwrap_or_default()
}

/// `english` in the interface's language.
pub fn t(english: &str) -> &str {
    translated(get(), english).unwrap_or(english)
}

/// `english` in `language`, when it isn't English and the interface's words, or the core's, have it.
pub fn translated(language: Language, english: &str) -> Option<&'static str> {
    if language == Language::English {
        return None;
    }
    // ponytail: a linear scan, a few hundred a frame; a map built once if it ever shows in a profile.
    WORDS.iter().find(|w| w[0] == english).map(|&w| language.of(w)).or_else(|| language.translated(english))
}

/// `template` translated, its `{}`s filled with `args` in order.
pub fn tf(template: &str, args: &[&str]) -> String {
    args.iter().fold(t(template).to_string(), |s, a| s.replacen("{}", a, 1))
}

/// An element category's name, of one (`Wall`) or of several (`Walls`).
pub fn category(category: &str, several: bool) -> String {
    let (one, many) = match category {
        "wall" => (["Wall", "Parede", "Muro"], ["Walls", "Paredes", "Muros"]),
        "door" => (["Door", "Porta", "Puerta"], ["Doors", "Portas", "Puertas"]),
        "window" => (["Window", "Janela", "Ventana"], ["Windows", "Janelas", "Ventanas"]),
        "floor" => (["Floor", "Piso", "Piso"], ["Floors", "Pisos", "Pisos"]),
        "room" => (["Room", "Ambiente", "Habitación"], ["Rooms", "Ambientes", "Habitaciones"]),
        "level" => (["Level", "Nível", "Nivel"], ["Levels", "Níveis", "Niveles"]),
        "grid" => (["Grid", "Eixo", "Eje"], ["Grids", "Eixos", "Ejes"]),
        "dimension" => (["Dimension", "Cota", "Cota"], ["Dimensions", "Cotas", "Cotas"]),
        "tag" => (["Tag", "Identificador", "Etiqueta"], ["Tags", "Identificadores", "Etiquetas"]),
        "section" => (["Section", "Corte", "Sección"], ["Sections", "Cortes", "Secciones"]),
        "elevation" => (["Elevation", "Elevação", "Elevación"], ["Elevations", "Elevações", "Elevaciones"]),
        "sheet" => (["Sheet", "Folha", "Plano"], ["Sheets", "Folhas", "Planos"]),
        "viewport" => (["Viewport", "Janela de vista", "Vista en plano"], ["Viewports", "Janelas de vista", "Vistas en plano"]),
        "column" => (["Column", "Pilar", "Columna"], ["Columns", "Pilares", "Columnas"]),
        "stair" => (["Stair", "Escada", "Escalera"], ["Stairs", "Escadas", "Escaleras"]),
        "roof" => (["Roof", "Telhado", "Cubierta"], ["Roofs", "Telhados", "Cubiertas"]),
        "text" => (["Text Note", "Nota de texto", "Nota de texto"], ["Text Notes", "Notas de texto", "Notas de texto"]),
        "spot" => (["Spot Elevation", "Elevação de ponto", "Cota de nivel"], ["Spot Elevations", "Elevações de ponto", "Cotas de nivel"]),
        "schedule" => (["Schedule", "Tabela", "Tabla"], ["Schedules", "Tabelas", "Tablas"]),
        "view3d" => (["3D View", "Vista 3D", "Vista 3D"], ["3D Views", "Vistas 3D", "Vistas 3D"]),
        other => return other.to_string(),
    };
    get().of(if several { many } else { one }).to_string()
}

/// The interface's words in English, Brazilian Portuguese and Spanish; the core's words (schedule
/// names and headings) besides.
const WORDS: &[[&str; 3]] = &[
    // The ribbon's tabs and the File menu.
    ["File", "Arquivo", "Archivo"],
    ["Architecture", "Arquitetura", "Arquitectura"],
    ["Annotate", "Anotar", "Anotar"],
    ["View", "Vista", "Vista"],
    ["Manage", "Gerenciar", "Gestionar"],
    ["Modify", "Modificar", "Modificar"],
    ["New", "Novo", "Nuevo"],
    ["Open…", "Abrir…", "Abrir…"],
    ["Save", "Salvar", "Guardar"],
    ["Save As…", "Salvar como…", "Guardar como…"],
    ["Export", "Exportar", "Exportar"],
    ["IFC…", "IFC…", "IFC…"],
    ["View as PNG…", "Vista como PNG…", "Vista como PNG…"],
    ["Plan as SVG…", "Planta como SVG…", "Planta como SVG…"],
    ["Sheets as PDF…", "Folhas como PDF…", "Planos como PDF…"],
    ["Exit", "Sair", "Salir"],
    ["BIMCraft project", "Projeto BIMCraft", "Proyecto BIMCraft"],
    ["BIMCraft project or IFC", "Projeto BIMCraft ou IFC", "Proyecto BIMCraft o IFC"],
    ["PNG image", "Imagem PNG", "Imagen PNG"],
    ["SVG drawing", "Desenho SVG", "Dibujo SVG"],
    // The Quick Access Toolbar and the navigation bar.
    ["Open", "Abrir", "Abrir"],
    ["Undo", "Desfazer", "Deshacer"],
    ["Redo", "Refazer", "Rehacer"],
    ["Default 3D View", "Vista 3D padrão", "Vista 3D predeterminada"],
    ["Zoom to Fit", "Zoom para ajustar", "Zoom para ajustar"],
    ["Zoom In", "Aproximar", "Acercar"],
    ["Zoom Out", "Afastar", "Alejar"],
    ["Home View", "Vista inicial", "Vista inicial"],
    // Panels.
    ["Select", "Selecionar", "Seleccionar"],
    ["Build", "Construir", "Construir"],
    ["Circulation", "Circulação", "Circulación"],
    ["Room & Area", "Ambiente e área", "Habitación y área"],
    ["Datum", "Referência", "Referencia"],
    ["Dimension", "Cota", "Cota"],
    ["Text", "Texto", "Texto"],
    ["Tag", "Identificar", "Etiquetar"],
    ["Graphics", "Gráficos", "Gráficos"],
    ["Create", "Criar", "Crear"],
    ["Sheet Composition", "Composição de folha", "Composición de planos"],
    ["Windows", "Janelas", "Ventanas"],
    ["Settings", "Configurações", "Configuración"],
    ["Properties", "Propriedades", "Propiedades"],
    // Tools and their tips.
    ["Modify (MD): select elements", "Modificar (MD): selecionar elementos", "Modificar (MD): seleccionar elementos"],
    [
        "Wall (WA): click points; Enter finishes, Esc cancels",
        "Parede (WA): clique nos pontos; Enter termina, Esc cancela",
        "Muro (WA): haz clic en los puntos; Enter termina, Esc cancela",
    ],
    ["Door", "Porta", "Puerta"],
    ["Door (DR): click a wall", "Porta (DR): clique numa parede", "Puerta (DR): haz clic en un muro"],
    ["Window", "Janela", "Ventana"],
    ["Window (WN): click a wall", "Janela (WN): clique numa parede", "Ventana (WN): haz clic en un muro"],
    ["Column", "Pilar", "Columna"],
    ["Column (CL): click where it stands", "Pilar (CL): clique onde ele fica", "Columna (CL): haz clic donde va"],
    ["Roof", "Telhado", "Cubierta"],
    [
        "Roof by Footprint: click inside the walls to roof them, on the level above",
        "Telhado por perímetro: clique dentro das paredes para cobri-las, no nível acima",
        "Cubierta por perímetro: haz clic dentro de los muros para cubrirlos, en el nivel superior",
    ],
    ["Stair", "Escada", "Escalera"],
    [
        "Stair: click where it starts, then which way it climbs",
        "Escada: clique onde ela começa e depois na direção em que sobe",
        "Escalera: haz clic donde empieza y luego hacia dónde sube",
    ],
    ["Room (RM): click inside walls", "Ambiente (RM): clique dentro das paredes", "Habitación (RM): haz clic dentro de los muros"],
    ["Level (LL): a new level at an elevation", "Nível (LL): um novo nível numa elevação", "Nivel (LL): un nivel nuevo a una elevación"],
    ["Grid", "Eixo", "Eje"],
    ["Grid (GR): click its two ends", "Eixo (GR): clique nas duas extremidades", "Eje (GR): haz clic en sus dos extremos"],
    ["Aligned", "Alinhada", "Alineada"],
    [
        "Aligned Dimension (DI): pick parallel wall faces, wall ends, doors, windows or grids, then click where the dimension line goes",
        "Cota alinhada (DI): selecione faces e extremidades de paredes, portas, janelas ou eixos paralelos e clique onde vai a linha de cota",
        "Cota alineada (DI): elige caras o extremos de muros, puertas, ventanas o ejes paralelos y haz clic donde va la línea de cota",
    ],
    ["Angular", "Angular", "Angular"],
    [
        "Angular Dimension: pick two lines that meet, then click where the arc goes",
        "Cota angular: selecione duas linhas que se encontram e clique onde vai o arco",
        "Cota angular: elige dos líneas que se encuentran y haz clic donde va el arco",
    ],
    ["Radial", "Radial", "Radial"],
    [
        "Radial Dimension: pick a round column, then click where the radius goes",
        "Cota radial: selecione um pilar redondo e clique onde vai o raio",
        "Cota radial: elige una columna redonda y haz clic donde va el radio",
    ],
    ["Spot Elevation", "Elevação de ponto", "Cota de nivel"],
    [
        "Spot Elevation (EL): click a floor, a roof or a stair; in a section, a point",
        "Elevação de ponto (EL): clique num piso, num telhado ou numa escada; num corte, num ponto",
        "Cota de nivel (EL): haz clic en un piso, una cubierta o una escalera; en una sección, en un punto",
    ],
    [
        "Text (TX): type the note in the Options Bar, then click where it goes",
        "Texto (TX): digite a nota na Barra de opções e clique onde ela vai",
        "Texto (TX): escribe la nota en la barra de opciones y haz clic donde va",
    ],
    ["Tag by Category", "Identificar por categoria", "Etiquetar por categoría"],
    [
        "Tag by Category (TG): click a door or window to tag it with its mark",
        "Identificar por categoria (TG): clique numa porta ou janela para identificá-la com sua marca",
        "Etiquetar por categoría (TG): haz clic en una puerta o ventana para etiquetarla con su marca",
    ],
    ["Tag All", "Identificar todos", "Etiquetar todo"],
    [
        "Tag every door and window in this view that has no tag",
        "Identificar as portas e janelas desta vista que não têm identificador",
        "Etiquetar las puertas y ventanas de esta vista que no tienen etiqueta",
    ],
    ["Thin Lines", "Linhas finas", "Líneas finas"],
    [
        "Thin Lines (TL): every line one pixel wide",
        "Linhas finas (TL): toda linha com um pixel de largura",
        "Líneas finas (TL): todas las líneas de un píxel de ancho",
    ],
    ["Dark Mode", "Modo escuro", "Modo oscuro"],
    [
        "Dark Mode: the window and the views light on dark; sheets stay paper. It starts as your system's theme",
        "Modo escuro: a janela e as vistas claras sobre fundo escuro; as folhas continuam papel. Começa no tema do sistema",
        "Modo oscuro: la ventana y las vistas, claras sobre oscuro; los planos siguen siendo papel. Empieza con el tema del sistema",
    ],
    ["Crop View", "Recortar vista", "Recortar vista"],
    [
        "Crop View: show only the crop region; drag its edges by their grips",
        "Recortar vista: mostrar só a região de recorte; arraste suas bordas pelas alças",
        "Recortar vista: mostrar solo la región de recorte; arrastra sus bordes por los tiradores",
    ],
    ["3D View", "Vista 3D", "Vista 3D"],
    ["Open the default 3D view", "Abrir a vista 3D padrão", "Abrir la vista 3D predeterminada"],
    [
        "Section: click its start, then its end; it looks to the left of the line",
        "Corte: clique no início e depois no fim; ele olha para a esquerda da linha",
        "Sección: haz clic en su inicio y luego en su final; mira hacia la izquierda de la línea",
    ],
    [
        "Elevation: click beside the building, on the side to look at",
        "Elevação: clique ao lado da construção, no lado a olhar",
        "Elevación: haz clic junto al edificio, en el lado que quieres ver",
    ],
    ["Schedules", "Tabelas", "Tablas"],
    ["Open a schedule", "Abrir uma tabela", "Abrir una tabla"],
    ["Sheet", "Folha", "Plano"],
    ["A new sheet, with a title block", "Uma nova folha, com carimbo", "Un plano nuevo, con cajetín"],
    ["Place View", "Inserir vista", "Colocar vista"],
    [
        "Put a view on this sheet: one not on a sheet yet",
        "Colocar nesta folha uma vista que ainda não está em nenhuma",
        "Poner en este plano una vista que aún no está en ninguno",
    ],
    ["Every view is on a sheet.", "Todas as vistas estão em folhas.", "Todas las vistas están en planos."],
    ["Close Inactive", "Fechar inativas", "Cerrar inactivas"],
    ["Close every view but this one", "Fechar todas as vistas menos esta", "Cerrar todas las vistas menos esta"],
    ["User Interface", "Interface do usuário", "Interfaz de usuario"],
    [
        "Show or hide the palettes, and pick the interface's language",
        "Mostrar ou ocultar as paletas, e escolher o idioma da interface",
        "Mostrar u ocultar las paletas y elegir el idioma de la interfaz",
    ],
    ["Project Browser", "Navegador de projeto", "Navegador de proyectos"],
    ["Language", "Idioma", "Idioma"],
    ["Project Units", "Unidades do projeto", "Unidades del proyecto"],
    [
        "Project Units (UN): how lengths show and read; now {}",
        "Unidades do projeto (UN): como os comprimentos aparecem e são lidos; agora {}",
        "Unidades del proyecto (UN): cómo se muestran y se leen las longitudes; ahora {}",
    ],
    ["Project Language", "Idioma do projeto", "Idioma del proyecto"],
    [
        "Project Language: what the drawings say and how new things are named; now {}",
        "Idioma do projeto: o que os desenhos dizem e como as coisas novas são nomeadas; agora {}",
        "Idioma del proyecto: lo que dicen los dibujos y cómo se nombran los elementos nuevos; ahora {}",
    ],
    ["Millimetres", "Milímetros", "Milímetros"],
    ["Centimetres", "Centímetros", "Centímetros"],
    ["Metres", "Metros", "Metros"],
    ["Inches", "Polegadas", "Pulgadas"],
    ["Feet", "Pés", "Pies"],
    ["Feet and inches", "Pés e polegadas", "Pies y pulgadas"],
    ["Show or hide the Properties palette", "Mostrar ou ocultar a paleta Propriedades", "Mostrar u ocultar la paleta Propiedades"],
    ["Type Properties", "Propriedades de tipo", "Propiedades de tipo"],
    ["Edit the type", "Editar o tipo", "Editar el tipo"],
    ["Align", "Alinhar", "Alinear"],
    [
        "Align (AL): pick a reference line, then what to line up with it",
        "Alinhar (AL): selecione uma linha de referência e depois o que alinhar com ela",
        "Alinear (AL): elige una línea de referencia y luego lo que se alinea con ella",
    ],
    ["Offset", "Deslocamento", "Desfase"],
    [
        "Offset (OF): pick a wall; a copy goes to the cursor's side",
        "Deslocamento (OF): selecione uma parede; uma cópia vai para o lado do cursor",
        "Desfase (OF): elige un muro; una copia va al lado del cursor",
    ],
    ["Mirror Pick Axis", "Espelhar - selecionar eixo", "Reflejar - elegir eje"],
    [
        "Mirror, Pick Axis (MM): pick a wall or grid to mirror across",
        "Espelhar, selecionar eixo (MM): selecione uma parede ou um eixo como espelho",
        "Reflejar, elegir eje (MM): elige un muro o un eje como eje de simetría",
    ],
    ["Mirror Draw Axis", "Espelhar - desenhar eixo", "Reflejar - dibujar eje"],
    [
        "Mirror, Draw Axis (DM): click the axis's two ends",
        "Espelhar, desenhar eixo (DM): clique nas duas extremidades do eixo",
        "Reflejar, dibujar eje (DM): haz clic en los dos extremos del eje",
    ],
    ["Split Element", "Dividir elemento", "Dividir elemento"],
    [
        "Split Element (SL): click a wall where to split it",
        "Dividir elemento (SL): clique numa parede onde dividi-la",
        "Dividir elemento (SL): haz clic en un muro donde dividirlo",
    ],
    ["Move", "Mover", "Mover"],
    [
        "Move (MV): click a base point, then where it goes",
        "Mover (MV): clique num ponto base e depois para onde ele vai",
        "Mover (MV): haz clic en un punto base y luego adonde va",
    ],
    ["Copy", "Copiar", "Copiar"],
    [
        "Copy (CO): click a base point, then where the copy goes",
        "Copiar (CO): clique num ponto base e depois onde vai a cópia",
        "Copiar (CO): haz clic en un punto base y luego donde va la copia",
    ],
    ["Rotate", "Rotacionar", "Rotar"],
    [
        "Rotate (RO): click the angle's start, then its end",
        "Rotacionar (RO): clique no início do ângulo e depois no fim",
        "Rotar (RO): haz clic en el inicio del ángulo y luego en su final",
    ],
    ["Trim/Extend to Corner", "Aparar/estender para canto", "Recortar/extender a esquina"],
    [
        "Trim/Extend to Corner (TR): pick two walls, on the parts to keep",
        "Aparar/estender para canto (TR): selecione duas paredes, nas partes a manter",
        "Recortar/extender a esquina (TR): elige dos muros, en las partes que se conservan",
    ],
    ["Delete", "Excluir", "Eliminar"],
    ["Delete (DE) the selection", "Excluir (DE) a seleção", "Eliminar (DE) la selección"],
    // The contextual tab.
    ["Place Wall", "Inserir parede", "Colocar muro"],
    ["Place Door", "Inserir porta", "Colocar puerta"],
    ["Place Window", "Inserir janela", "Colocar ventana"],
    ["Place Room", "Inserir ambiente", "Colocar habitación"],
    ["Place Level", "Inserir nível", "Colocar nivel"],
    ["Place Grid", "Inserir eixo", "Colocar eje"],
    ["Place Dimensions", "Inserir cotas", "Colocar cotas"],
    ["Place Text", "Inserir texto", "Colocar texto"],
    ["Place Column", "Inserir pilar", "Colocar columna"],
    ["Create Stair", "Criar escada", "Crear escalera"],
    ["Create Roof Footprint", "Criar perímetro do telhado", "Crear perímetro de cubierta"],
    ["Multi-Select", "Seleção múltipla", "Selección múltiple"],
    // The Options Bar.
    ["Height:", "Altura:", "Altura:"],
    ["Unconnected", "Desconectada", "No conectada"],
    ["Chain", "Cadeia", "Cadena"],
    ["Width:", "Largura:", "Ancho:"],
    ["Slope:", "Inclinação:", "Pendiente:"],
    ["Overhang:", "Beiral:", "Voladizo:"],
    ["Gable", "Empena", "Hastial"],
    ["Constrain", "Restringir", "Restringir"],
    ["Multiple", "Múltiplo", "Múltiple"],
    ["Leader", "Linha de chamada", "Directriz"],
    ["Text:", "Texto:", "Texto:"],
    ["what the note says", "o que a nota diz", "lo que dice la nota"],
    ["Offset:", "Deslocamento:", "Desfase:"],
    ["Angle:", "Ângulo:", "Ángulo:"],
    ["Place Centre", "Posicionar centro", "Colocar centro"],
    ["the next free name", "o próximo nome livre", "el siguiente nombre libre"],
    // The status bar's prompts.
    [
        "Click to select. Drag to orbit (or Shift and the middle button), the middle button pans, the wheel zooms.",
        "Clique para selecionar. Arraste para orbitar (ou Shift e o botão do meio); o botão do meio move a vista, a roda aproxima.",
        "Haz clic para seleccionar. Arrastra para orbitar (o Shift y el botón central); el botón central desplaza la vista, la rueda hace zoom.",
    ],
    [
        "Click a view on the sheet to select it, drag it to move it, double-click it to open it.",
        "Clique numa vista da folha para selecioná-la, arraste para movê-la, clique duas vezes para abri-la.",
        "Haz clic en una vista del plano para seleccionarla, arrástrala para moverla y haz doble clic para abrirla.",
    ],
    [
        "Click to select what the view cuts or sees; the middle button pans, the wheel zooms.",
        "Clique para selecionar o que a vista corta ou vê; o botão do meio move a vista, a roda aproxima.",
        "Haz clic para seleccionar lo que la vista corta o ve; el botón central desplaza la vista, la rueda hace zoom.",
    ],
    [
        "Type the new level's elevation in the Options Bar, then Create.",
        "Digite a elevação do novo nível na Barra de opções e clique em Criar.",
        "Escribe la elevación del nivel nuevo en la barra de opciones y luego Crear.",
    ],
    [
        "Type the note in the Options Bar, then click where it goes.",
        "Digite a nota na Barra de opções e clique onde ela vai.",
        "Escribe la nota en la barra de opciones y haz clic donde va.",
    ],
    ["Click a point to mark its height.", "Clique num ponto para marcar sua altura.", "Haz clic en un punto para marcar su altura."],
    [
        "This tool works in plan views: open one from the Project Browser.",
        "Esta ferramenta funciona em plantas: abra uma pelo Navegador de projeto.",
        "Esta herramienta funciona en plantas: abre una desde el navegador de proyectos.",
    ],
    [
        "Click to select, Tab for what's underneath, Ctrl adds, Shift takes away; drag to box or to move.",
        "Clique para selecionar, Tab para o que está embaixo, Ctrl adiciona, Shift remove; arraste para uma caixa ou para mover.",
        "Haz clic para seleccionar, Tab para lo que hay debajo, Ctrl agrega, Shift quita; arrastra para un recuadro o para mover.",
    ],
    [
        "Click to enter the wall's start point.",
        "Clique para indicar o ponto inicial da parede.",
        "Haz clic para indicar el punto inicial del muro.",
    ],
    [
        "Click to enter the next point. Enter finishes, Esc cancels, Shift keeps it square.",
        "Clique para indicar o próximo ponto. Enter termina, Esc cancela, Shift mantém no esquadro.",
        "Haz clic para indicar el siguiente punto. Enter termina, Esc cancela, Shift lo mantiene a escuadra.",
    ],
    ["Click a wall to place the door.", "Clique numa parede para inserir a porta.", "Haz clic en un muro para colocar la puerta."],
    ["Click a wall to place the window.", "Clique numa parede para inserir a janela.", "Haz clic en un muro para colocar la ventana."],
    [
        "Click inside walls to place a room; the outline shows the room you'd get.",
        "Clique dentro das paredes para inserir um ambiente; o contorno mostra o ambiente resultante.",
        "Haz clic dentro de los muros para colocar una habitación; el contorno muestra la habitación que resultaría.",
    ],
    [
        "Click to enter the grid's start point.",
        "Clique para indicar o ponto inicial do eixo.",
        "Haz clic para indicar el punto inicial del eje.",
    ],
    ["Click to enter the grid's end point.", "Clique para indicar o ponto final do eixo.", "Haz clic para indicar el punto final del eje."],
    ["Click a base point to measure from.", "Clique num ponto base de referência.", "Haz clic en un punto base desde el que medir."],
    [
        "Click where the base point goes, or type a distance and press Enter. Esc cancels.",
        "Clique onde vai o ponto base, ou digite uma distância e pressione Enter. Esc cancela.",
        "Haz clic donde va el punto base, o escribe una distancia y presiona Enter. Esc cancela.",
    ],
    [
        "Click where the copy's base point goes, or type a distance and press Enter. Esc cancels.",
        "Clique onde vai o ponto base da cópia, ou digite uma distância e pressione Enter. Esc cancela.",
        "Haz clic donde va el punto base de la copia, o escribe una distancia y presiona Enter. Esc cancela.",
    ],
    ["Click the centre of rotation.", "Clique no centro de rotação.", "Haz clic en el centro de rotación."],
    [
        "Click the start of the angle, or type an angle in the Options Bar.",
        "Clique no início do ângulo, ou digite um ângulo na Barra de opções.",
        "Haz clic en el inicio del ángulo, o escribe un ángulo en la barra de opciones.",
    ],
    ["Click the end of the angle. Esc cancels.", "Clique no fim do ângulo. Esc cancela.", "Haz clic en el final del ángulo. Esc cancela."],
    [
        "Pick a wall or grid to mirror across.",
        "Selecione uma parede ou um eixo como espelho.",
        "Elige un muro o un eje como eje de simetría.",
    ],
    ["Click the start of the mirror axis.", "Clique no início do eixo de espelhamento.", "Haz clic en el inicio del eje de simetría."],
    ["Click the end of the mirror axis.", "Clique no fim do eixo de espelhamento.", "Haz clic en el final del eje de simetría."],
    [
        "Pick a reference line: a grid, or a wall's face or centre line.",
        "Selecione uma linha de referência: um eixo, ou a face ou a linha de centro de uma parede.",
        "Elige una línea de referencia: un eje, o la cara o la línea central de un muro.",
    ],
    [
        "Pick the wall or grid to line up with the reference.",
        "Selecione a parede ou o eixo a alinhar com a referência.",
        "Elige el muro o el eje que se alinea con la referencia.",
    ],
    [
        "Pick the first wall, on the part to keep.",
        "Selecione a primeira parede, na parte a manter.",
        "Elige el primer muro, en la parte que se conserva.",
    ],
    [
        "Pick the second wall, on the part to keep.",
        "Selecione a segunda parede, na parte a manter.",
        "Elige el segundo muro, en la parte que se conserva.",
    ],
    ["Click a wall where to split it.", "Clique numa parede onde dividi-la.", "Haz clic en un muro donde dividirlo."],
    [
        "Point to a wall: the offset goes to the cursor's side. Click to place it.",
        "Aponte para uma parede: o deslocamento vai para o lado do cursor. Clique para inseri-lo.",
        "Señala un muro: el desfase va al lado del cursor. Haz clic para colocarlo.",
    ],
    [
        "Pick a wall face, centre line or end, a door, a window or a grid to measure from.",
        "Selecione a face, a linha de centro ou a extremidade de uma parede, uma porta, uma janela ou um eixo para medir.",
        "Elige la cara, la línea central o el extremo de un muro, una puerta, una ventana o un eje desde el que medir.",
    ],
    [
        "Pick the next reference, parallel to the first.",
        "Selecione a próxima referência, paralela à primeira.",
        "Elige la siguiente referencia, paralela a la primera.",
    ],
    [
        "Pick more references, or click away from them where the dimension line goes. Esc cancels.",
        "Selecione mais referências, ou clique longe delas onde vai a linha de cota. Esc cancela.",
        "Elige más referencias, o haz clic lejos de ellas donde va la línea de cota. Esc cancela.",
    ],
    [
        "Click a door or window to tag it with its mark.",
        "Clique numa porta ou janela para identificá-la com sua marca.",
        "Haz clic en una puerta o ventana para etiquetarla con su marca.",
    ],
    [
        "Pick a wall, a grid or another line; then one that meets it.",
        "Selecione uma parede, um eixo ou outra linha; depois uma que a encontre.",
        "Elige un muro, un eje u otra línea; luego una que se encuentre con ella.",
    ],
    [
        "Click where the arc goes: the angle on that side. Esc cancels.",
        "Clique onde vai o arco: o ângulo desse lado. Esc cancela.",
        "Haz clic donde va el arco: el ángulo de ese lado. Esc cancela.",
    ],
    ["Pick a round column.", "Selecione um pilar redondo.", "Elige una columna redonda."],
    ["Click where the radius goes. Esc cancels.", "Clique onde vai o raio. Esc cancela.", "Haz clic donde va el radio. Esc cancela."],
    [
        "Type the note in the Options Bar, then click where it goes: its top left corner.",
        "Digite a nota na Barra de opções e clique onde ela vai: seu canto superior esquerdo.",
        "Escribe la nota en la barra de opciones y haz clic donde va: su esquina superior izquierda.",
    ],
    [
        "Click a floor, a roof, a stair or a wall: the spot elevation reads the height of its top there.",
        "Clique num piso, num telhado, numa escada ou numa parede: a elevação de ponto lê a altura do topo ali.",
        "Haz clic en un piso, una cubierta, una escalera o un muro: la cota de nivel lee la altura de su parte superior ahí.",
    ],
    [
        "Click the section's start: it looks to the left of the line you draw.",
        "Clique no início do corte: ele olha para a esquerda da linha desenhada.",
        "Haz clic en el inicio de la sección: mira hacia la izquierda de la línea que dibujes.",
    ],
    ["Click the section's end.", "Clique no fim do corte.", "Haz clic en el final de la sección."],
    [
        "Click beside the building: the elevation looks at the side you click by.",
        "Clique ao lado da construção: a elevação olha para o lado onde você clicar.",
        "Haz clic junto al edificio: la elevación mira el lado junto al que hagas clic.",
    ],
    [
        "Click to place a column, as high as the Options Bar says.",
        "Clique para inserir um pilar, da altura que a Barra de opções indica.",
        "Haz clic para colocar una columna, tan alta como indica la barra de opciones.",
    ],
    [
        "Click where the stair starts: the middle of its first riser.",
        "Clique onde a escada começa: o meio do primeiro espelho.",
        "Haz clic donde empieza la escalera: el centro de su primera contrahuella.",
    ],
    [
        "Click which way it climbs: its risers take it up to the level above. Esc cancels.",
        "Clique na direção em que ela sobe: os espelhos a levam ao nível acima. Esc cancela.",
        "Haz clic hacia donde sube: sus contrahuellas la llevan al nivel superior. Esc cancela.",
    ],
    [
        "Click inside the walls to roof them, on the level above: the overhang runs past their faces.",
        "Clique dentro das paredes para cobri-las, no nível acima: o beiral passa das faces.",
        "Haz clic dentro de los muros para cubrirlos, en el nivel superior: el voladizo sobresale de sus caras.",
    ],
    // Messages the window writes itself.
    [
        "This project has no levels. Add one with the Level tool.",
        "Este projeto não tem níveis. Adicione um com a ferramenta Nível.",
        "Este proyecto no tiene niveles. Agrega uno con la herramienta Nivel.",
    ],
    [
        "select what to move, copy, rotate or mirror first",
        "selecione primeiro o que mover, copiar, rotacionar ou espelhar",
        "selecciona primero lo que quieres mover, copiar, rotar o reflejar",
    ],
    ["`{}` is not a height", "`{}` não é uma altura", "`{}` no es una altura"],
    ["rotate: `{}` is not an angle in degrees", "rotacionar: `{}` não é um ângulo em graus", "rotar: `{}` no es un ángulo en grados"],
    ["level: `{}` is not an elevation", "nível: `{}` não é uma elevação", "nivel: `{}` no es una elevación"],
    ["column: {}", "pilar: {}", "columna: {}"],
    ["wall: {}", "parede: {}", "muro: {}"],
    ["stair: `{}` is not a width", "escada: `{}` não é uma largura", "escalera: `{}` no es un ancho"],
    [
        "roof: the slope `{}` and the overhang `{}` must be numbers",
        "telhado: a inclinação `{}` e o beiral `{}` precisam ser números",
        "cubierta: la pendiente `{}` y el voladizo `{}` deben ser números",
    ],
    [
        "text: type what the note says in the Options Bar first",
        "texto: digite primeiro na Barra de opções o que a nota diz",
        "texto: escribe primero en la barra de opciones lo que dice la nota",
    ],
    ["align: the lines aren't parallel", "alinhar: as linhas não são paralelas", "alinear: las líneas no son paralelas"],
    ["offset: `{}` is not a distance", "deslocamento: `{}` não é uma distância", "desfase: `{}` no es una distancia"],
    ["`{}` is not a length", "`{}` não é um comprimento", "`{}` no es una longitud"],
    [
        "this tool works in plan views: open one from the Project Browser",
        "esta ferramenta funciona em plantas: abra uma pelo Navegador de projeto",
        "esta herramienta funciona en plantas: abre una desde el navegador de proyectos",
    ],
    ["`{}` is not a number", "`{}` não é um número", "`{}` no es un número"],
    ["`{}` is not a point; type x, y", "`{}` não é um ponto; digite x, y", "`{}` no es un punto; escribe x, y"],
    ["`{}` is not a point; type x, y in mm", "`{}` não é um ponto; digite x, y em mm", "`{}` no es un punto; escribe x, y en mm"],
    ["{}: `{}` is not a length", "{}: `{}` não é um comprimento", "{}: `{}` no es una longitud"],
    ["Imported {}: {}.", "Importado {}: {}.", "Importado {}: {}."],
    [" Left out: {}{}.", " Deixado de fora: {}{}.", " Quedó fuera: {}{}."],
    ["; and {} more", "; e mais {}", "; y {} más"],
    [
        " Moved {}, {} mm to bring it near the origin.",
        " Movido {}, {} mm para trazê-lo perto da origem.",
        " Movido {}, {} mm para acercarlo al origen.",
    ],
    [
        "Not enclosed: the walls around it don't close.",
        "Não delimitado: as paredes em volta não se fecham.",
        "No delimitada: los muros que la rodean no se cierran.",
    ],
    ["Temporary dimension {}", "Cota temporária {}", "Cota temporal {}"],
    // The status bar, the warning box, the unsaved-changes dialog.
    ["Unsaved changes", "Alterações não salvas", "Cambios sin guardar"],
    ["Error", "Erro", "Error"],
    ["Note", "Nota", "Nota"],
    ["Dismiss", "Dispensar", "Descartar"],
    ["Save changes to {}?", "Salvar as alterações em {}?", "¿Guardar los cambios en {}?"],
    [
        "If you don't, the changes since it was last saved are lost.",
        "Se não salvar, as alterações desde a última vez em que foi salvo serão perdidas.",
        "Si no lo haces, se pierden los cambios desde la última vez que se guardó.",
    ],
    ["Don't Save", "Não salvar", "No guardar"],
    ["Cancel", "Cancelar", "Cancelar"],
    // Views: their kinds, the Project Browser, the view tabs, Place View.
    ["Floor Plan", "Planta de piso", "Planta"],
    ["Schedule", "Tabela", "Tabla"],
    ["{}: {}", "{}: {}", "{}: {}"],
    ["Floor Plan: {}", "Planta de piso: {}", "Planta: {}"],
    ["Section: {}", "Corte: {}", "Sección: {}"],
    ["Elevation: {}", "Elevação: {}", "Elevación: {}"],
    ["3D View: {3D}", "Vista 3D: {3D}", "Vista 3D: {3D}"],
    ["Schedule: {}", "Tabela: {}", "Tabla: {}"],
    ["Project Browser · {}", "Navegador de projeto · {}", "Navegador de proyectos · {}"],
    ["Views (all)", "Vistas (todas)", "Vistas (todas)"],
    ["Floor Plans", "Plantas de piso", "Plantas"],
    ["3D Views", "Vistas 3D", "Vistas 3D"],
    ["Elevations (Building Elevation)", "Elevações (elevação da construção)", "Elevaciones (elevación del edificio)"],
    ["Sections (Building Section)", "Cortes (corte da construção)", "Secciones (sección del edificio)"],
    ["Schedules/Quantities (all)", "Tabelas/quantitativos (todas)", "Tablas/cantidades (todas)"],
    ["Sheets (all)", "Folhas (todas)", "Planos (todos)"],
    ["Close {}", "Fechar {}", "Cerrar {}"],
    ["No rows yet: the {} is empty.", "Ainda sem linhas: a {} está vazia.", "Aún no hay filas: la {} está vacía."],
    // Properties and Type Properties.
    ["Edit Type", "Editar tipo", "Editar tipo"],
    ["Apply", "Aplicar", "Aplicar"],
    ["Type Selector", "Seletor de tipo", "Selector de tipo"],
    ["Multiple Categories", "Várias categorias", "Varias categorías"],
    ["Multiple Categories ({})", "Várias categorias ({})", "Varias categorías ({})"],
    ["New {}", "Inserir: {}", "Colocar: {}"],
    ["{} ({})", "{} ({})", "{} ({})"],
    ["Constraints", "Restrições", "Restricciones"],
    ["Dimensions", "Dimensões", "Dimensiones"],
    ["Identity Data", "Dados de identidade", "Datos de identidad"],
    ["Other", "Outros", "Otros"],
    ["Extents", "Extensões", "Extensión"],
    ["View Range", "Intervalo da vista", "Rango de vista"],
    ["Camera", "Câmera", "Cámara"],
    ["Base Constraint", "Restrição da base", "Restricción de base"],
    ["Base Offset", "Deslocamento da base", "Desfase de base"],
    ["Top Constraint", "Restrição superior", "Restricción superior"],
    ["Unconnected Height", "Altura desconectada", "Altura no conectada"],
    ["Start", "Início", "Inicio"],
    ["End", "Fim", "Final"],
    ["Host Wall", "Parede hospedeira", "Muro anfitrión"],
    ["Flip Facing", "Inverter face", "Invertir orientación"],
    ["Flip Hand", "Inverter lado", "Invertir lado"],
    ["Sill Height", "Altura do peitoril", "Altura de antepecho"],
    ["Area", "Área", "Área"],
    ["Perimeter", "Perímetro", "Perímetro"],
    ["Location", "Localização", "Ubicación"],
    ["Value", "Valor", "Valor"],
    ["Kind", "Tipo de cota", "Tipo de cota"],
    ["Line Through", "Linha por", "Línea por"],
    ["Text Size (mm)", "Tamanho do texto (mm)", "Tamaño del texto (mm)"],
    ["Rotation (°)", "Rotação (°)", "Rotación (°)"],
    ["Host", "Hospedeiro", "Anfitrión"],
    ["View Name", "Nome da vista", "Nombre de la vista"],
    ["Category", "Categoria", "Categoría"],
    ["Azimuth (°)", "Azimute (°)", "Acimut (°)"],
    ["Elevation (°)", "Elevação (°)", "Elevación (°)"],
    ["Leader Line", "Linha de chamada", "Directriz"],
    ["Tagged Element", "Elemento identificado", "Elemento etiquetado"],
    ["Sheet Number", "Número da folha", "Número de plano"],
    ["Sheet Name", "Nome da folha", "Nombre de plano"],
    ["Size", "Tamanho", "Tamaño"],
    ["View Scale (1 : n)", "Escala da vista (1 : n)", "Escala de la vista (1 : n)"],
    ["Base Level", "Nível da base", "Nivel de base"],
    ["Top Level", "Nível superior", "Nivel superior"],
    ["Actual Run Width", "Largura real do lance", "Ancho real del tramo"],
    ["Maximum Riser Height", "Altura máxima do espelho", "Altura máxima de contrahuella"],
    ["Actual Tread Depth", "Profundidade real do piso", "Profundidad real de huella"],
    ["Actual Number of Risers", "Número real de espelhos", "Número real de contrahuellas"],
    ["Actual Riser Height", "Altura real do espelho", "Altura real de contrahuella"],
    ["Slope (°)", "Inclinação (°)", "Pendiente (°)"],
    ["Max Ridge Height", "Altura máxima da cumeeira", "Altura máxima de cumbrera"],
    ["Gable Ends", "Empenas", "Hastiales"],
    ["Far Clip Offset", "Deslocamento do recorte distante", "Desfase del plano lejano"],
    ["Associated Level", "Nível associado", "Nivel asociado"],
    ["Level Elevation", "Elevação do nível", "Elevación del nivel"],
    ["Top", "Superior", "Superior"],
    ["Cut Plane", "Plano de corte", "Plano de corte"],
    ["Bottom", "Inferior", "Inferior"],
    ["View Depth", "Profundidade da vista", "Profundidad de vista"],
    ["Level Above", "Nível acima", "Nivel de arriba"],
    ["Default Thickness", "Espessura padrão", "Espesor predeterminado"],
    ["Default Sill Height", "Altura padrão do peitoril", "Altura de antepecho predeterminada"],
    ["Depth", "Profundidade", "Profundidad"],
    ["Yes", "Sim", "Sí"],
    ["No", "Não", "No"],
    ["Family:", "Família:", "Familia:"],
    ["Type:", "Tipo:", "Tipo:"],
    ["Duplicate…", "Duplicar…", "Duplicar…"],
    ["New type", "Novo tipo", "Tipo nuevo"],
    ["Type Parameters", "Parâmetros de tipo", "Parámetros de tipo"],
    ["Parameter", "Parâmetro", "Parámetro"],
    ["OK", "OK", "Aceptar"],
    // Walls: how they're drawn, the line they stand on, what they're attached to, their layers.
    ["Draw", "Desenhar", "Dibujar"],
    ["Line", "Linha", "Línea"],
    [
        "Line: a wall along each step between the points clicked",
        "Linha: uma parede em cada trecho entre os pontos clicados",
        "Línea: un muro en cada tramo entre los puntos marcados",
    ],
    ["Start-End-Radius Arc", "Arco início-fim-raio", "Arco inicio-fin-radio"],
    [
        "Start-End-Radius Arc: click its start, its end, then a point it passes through",
        "Arco início-fim-raio: clique no início, no fim e depois num ponto por onde ele passa",
        "Arco inicio-fin-radio: haz clic en su inicio, en su final y luego en un punto por el que pasa",
    ],
    ["Modify Wall", "Modificar parede", "Modificar muro"],
    ["Flip", "Inverter", "Invertir"],
    [
        "Flip (Space): the walls' exterior sides go to the other side of their location lines",
        "Inverter (Espaço): o lado externo das paredes passa para o outro lado da linha de localização",
        "Invertir (Espacio): el lado exterior de los muros pasa al otro lado de su línea de ubicación",
    ],
    ["Attach Top/Base", "Anexar topo/base", "Unir parte superior/base"],
    [
        "Attach Top/Base: pick a roof or a floor for the walls' tops, or bases, to follow",
        "Anexar topo/base: selecione um telhado ou um piso para o topo das paredes, ou a base, acompanhar",
        "Unir parte superior/base: elige una cubierta o un piso para que lo siga la parte superior de los muros, o su base",
    ],
    ["Detach Top/Base", "Desanexar topo/base", "Separar parte superior/base"],
    [
        "Detach Top/Base: the walls' tops and bases their own again",
        "Desanexar topo/base: o topo e a base das paredes voltam a ser os seus",
        "Separar parte superior/base: la parte superior y la base de los muros vuelven a ser las suyas",
    ],
    ["Location Line:", "Linha de localização:", "Línea de ubicación:"],
    ["Location Line", "Linha de localização", "Línea de ubicación"],
    ["Wall Centreline", "Linha de centro da parede", "Línea central del muro"],
    ["Core Centreline", "Linha de centro do núcleo", "Línea central del núcleo"],
    ["Finish Face: Exterior", "Face de acabamento: externa", "Cara de acabado: exterior"],
    ["Finish Face: Interior", "Face de acabamento: interna", "Cara de acabado: interior"],
    ["Core Face: Exterior", "Face do núcleo: externa", "Cara del núcleo: exterior"],
    ["Core Face: Interior", "Face do núcleo: interna", "Cara del núcleo: interior"],
    ["Attach Wall:", "Anexar parede:", "Unir muro:"],
    ["Base", "Base", "Base"],
    ["select the walls to attach first", "selecione primeiro as paredes a anexar", "selecciona primero los muros que quieres unir"],
    ["offset: it offsets straight walls", "deslocamento: só desloca paredes retas", "desfase: solo desfasa muros rectos"],
    ["Click to enter the arc's end point.", "Clique para indicar o ponto final do arco.", "Haz clic para indicar el punto final del arco."],
    [
        "Click a point the arc passes through: it sets the radius. Esc cancels.",
        "Clique num ponto por onde o arco passa: ele define o raio. Esc cancela.",
        "Haz clic en un punto por el que pasa el arco: define el radio. Esc cancela.",
    ],
    [
        "Pick a roof or a floor: the selected walls' bases stand on it.",
        "Selecione um telhado ou um piso: a base das paredes selecionadas apoia-se nele.",
        "Elige una cubierta o un piso: la base de los muros seleccionados se apoya en él.",
    ],
    [
        "Pick a roof or a floor: the selected walls' tops follow its underside.",
        "Selecione um telhado ou um piso: o topo das paredes selecionadas acompanha a face inferior dele.",
        "Elige una cubierta o un piso: la parte superior de los muros seleccionados sigue su cara inferior.",
    ],
    ["Base is Attached", "Base anexada", "Base unida"],
    ["Top is Attached", "Topo anexado", "Parte superior unida"],
    ["Radius", "Raio", "Radio"],
    ["Layers", "Camadas", "Capas"],
    ["Function", "Função", "Función"],
    ["Material", "Material", "Material"],
    ["Out", "Para fora", "Hacia afuera"],
    ["In", "Para dentro", "Hacia adentro"],
    ["Insert Layer", "Inserir camada", "Insertar capa"],
    ["Structure", "Estrutura", "Estructura"],
    ["Substrate", "Substrato", "Sustrato"],
    ["Thermal", "Térmica", "Térmica"],
    ["Membrane", "Membrana", "Membrana"],
    ["Finish", "Acabamento", "Acabado"],
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_is_its_own_key_and_the_others_fall_back_to_the_cores_words() {
        set(Language::English);
        assert_eq!((t("Wall Schedule"), t("Wall")), ("Wall Schedule", "Wall"));
        set(Language::Portuguese);
        assert_eq!((t("Wall Schedule"), t("Arquitetura"), t("Architecture")), ("Tabela de paredes", "Arquitetura", "Arquitetura"));
        assert_eq!(
            tf("roof: the slope `{}` and the overhang `{}` must be numbers", &["x", "y"]),
            "telhado: a inclinação `x` e o beiral `y` precisam ser números"
        );
        assert_eq!((category("wall", true), category("spot", false)), ("Paredes".to_string(), "Elevação de ponto".to_string()));
        set(Language::Spanish);
        assert_eq!((t("Wall Schedule"), t("Save changes to {}?")), ("Tabla de muros", "¿Guardar los cambios en {}?"));
        assert_eq!((category("room", true), category("viewport", false)), ("Habitaciones".to_string(), "Vista en plano".to_string()));
        set(Language::English);
    }
}
