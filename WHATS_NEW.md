# CADCraft — что нового / What's new

Изменения после версии 0.9.0. Новые записи добавляются сверху, с датой.
Пожалуйста, попробуйте описанное и напишите, что работает не так, чего не хватает и какие у вас есть идеи.

Changes since version 0.9.0. New entries go on top, with a date.
Please try what is described here and tell us what doesn't work as expected, what is missing, and what ideas you have.

---

## 22.10.2026 (0.9.0-b12)

### Русский

**Свойства текста и выносок**
- В палитре «Свойства» у текста, многострочного текста и мультивыносок появилась строка «Стиль» (текстовый стиль). Она работает и для нескольких выбранных объектов сразу. Если стили разные, показывается «*РАЗНЫЕ*».
- В списках стрелок (у мультивыносок в «Свойствах» и в редакторе размерного стиля) рядом с каждой стрелкой нарисован её вид, в том числе у пользовательских стрелок-блоков.

**Ручка текста мультивыноски**
- У текста мультивыноски появилась своя ручка. Её меню: «Переместить с выноской», «Переместить только текст», «Восстановить положение текста».
- «Переместить с выноской» ставит текст в указанную точку, полочка встаёт рядом с текстом, а выноска тянется к ней от своей последней точки.
- «Переместить только текст» двигает один текст. «Восстановить положение текста» возвращает его к полочке.

**Печать: толщина линий**
- Найдена причина слишком тонких линий. У листов, настроенных в миллиметрах внутри дюймового чертежа (как в «אריה צוק»), PDF получался в 25,4 раза больше листа: A3 был 10,7 × 7,5 м. При печати такой страницы на A3 всё уменьшалось в 25,4 раза, вместе с толщиной линий. Теперь страница PDF имеет размер листа, и толщины линий печатаются как задано.
- Программа читает таблицы стилей печати AutoCAD (.ctb и .stb): цвета, толщины линий, оттенки серого, интенсивность. Если лист ссылается на таблицу, которой нет в программе (у «אריה צוק» это SIVAN.ctb), CADCraft ищет файл рядом с чертежом и в папках стилей печати AutoCAD этого пользователя. Если файл не найден, в командной строке появляется сообщение, и лист печатается с цветами и толщинами самих объектов. Файл .ctb также можно загрузить вручную в «Диспетчере стилей печати».

**Значки**
- «Показать целиком» в предварительном просмотре: четыре прямых угла вокруг прямоугольника.
- «Параметры»: шестерня с зубцами.
- «Отменить» и «Повторить»: толще дуга и крупнее стрелка.
- На панель инструментов можно добавить кнопки «Текстовый стиль», «Размерный стиль» и «Параметры» («Настроить панель инструментов», раздел «Стандартный»).

**Диспетчер блоков**
- Кнопка «Вставить блок» (и команда ВСТАВИТЬ / INSERT) открывает диспетчер блоков панелью, закреплённой справа, рядом со «Слоями» и «Свойствами».
- Значок в правом верхнем углу превращает панель в отдельное окно, а в окне — обратно в панель. Программа запоминает, как диспетчер открывался в последний раз.
- Команда -ВСТАВИТЬ (-INSERT) по-прежнему спрашивает имя блока в командной строке.

### English

**Text and leader properties**
- The Properties palette has a Style row (text style) for text, multiline text and multileaders. It works for several selected objects at once and shows *VARIES* when their styles differ.
- The arrowhead lists (multileaders in Properties, the dimension style editor) show a picture of each arrowhead, user arrowhead blocks included.

**The multileader text grip**
- Multileader text has its own grip, with Move with Leader, Move Text Only and Reset Text Position.
- Move with Leader puts the text at the picked point; the landing goes beside the text and the leader runs to it from its last point.
- Move Text Only moves the text alone; Reset Text Position puts it back beside the landing.

**Printing: lineweights**
- Found why lines printed too thin: a layout set up in millimetres in an inch drawing (as in "אריה צוק") produced a PDF page 25.4 times the sheet (A3 came out 10.7 × 7.5 m), so printing it on A3 shrank everything, lineweights included, 25.4 times. The PDF page is now the size of the sheet and lineweights print as set.
- AutoCAD plot style tables (.ctb and .stb) are read: colours, lineweights, grayscale and screening. When a layout names a table CADCraft doesn't have (SIVAN.ctb for "אריה צוק"), it looks next to the drawing and in this user's AutoCAD plot style folders. If it isn't found, the command line says so and the layout plots with the objects' own colours and lineweights. A .ctb file can also be loaded in the Plot Style Manager.

**Icons**
- Zoom to Fit in Plot Preview: four right-angle corners around a rectangle.
- Options: a gear with teeth.
- Undo and Redo: a thicker arc and a larger arrowhead.
- Text Style, Dimension Style and Options buttons can be added to the Tool Bar (Customize Tool Bar, Standard section).

**Block Manager**
- The Insert Block button (and INSERT typed) opens the Block Manager as a panel pinned on the right, beside Layers and Properties.
- The icon at its top right turns the panel into a window, and the window back into a panel. The last choice is remembered.
- -INSERT still asks for the block name on the command line.

---

## 21.10.2026 (0.9.0-b11)

### Русский

**Меню приложения (кнопка CADCraft)**
- Строки стали выше, текст крупнее. Слева от каждого пункта крупный значок.

**Значки**
- «Сохранить как»: крупный карандаш рядом с дискетой.
- «Импорт» и «Экспорт»: крупные стрелки.

**Текст (TEXT) и мультивыноска (MLEADER)**
- Пока вы указываете точку вставки, высоту и угол текста, рядом с курсором видна рамка, которую займёт строка.
- После этого открывается окно редактирования текста прямо на чертеже, как у многострочного текста. Enter завершает строку, и окно открывается для следующей строки под ней. Enter в пустой строке завершает команду.
- У мультивыноски, пока вы указываете точки, виден отрезок до курсора. После последней точки открывается окно редактирования текста.
- В палитре «Свойства» у мультивыноски появился тип выноски «Нет». Линия выноски и полка скрываются, но не удаляются: если снова выбрать «Прямая» или «Сплайн», выноска вернётся. В файл DXF записывается тип выноски «невидимая».

**Диспетчер блоков**
- Кнопка «Блоки» открывает диспетчер блоков: слева список блоков чертежа с поиском, справа предпросмотр выбранного блока, число объектов и вхождений, базовая точка, описание.
- Кнопки: «Вставить» (или двойной щелчок по блоку), «Редактировать блок», «Переименовать», «Удалить» (только неиспользуемый блок), «Сохранить в файл» (WBLOCK), «Создать».

**Предварительный просмотр печати**
- Сверху кнопки «Увеличить» и «Уменьшить» (лупа с плюсом и с минусом) и «Показать целиком». Колёсико мыши увеличивает и уменьшает изображение вокруг курсора.
- Лист можно перетаскивать мышью. Когда изображение больше окна, снизу и справа появляются полосы прокрутки.
- Окно можно растягивать. Изображение рисуется в более высоком разрешении.

**Пакетная печать — менеджер печати**
- Пакетная печать открывается в отдельном окне. В нём список листов текущего чертежа в порядке вкладок.
- Порядок меняется перетаскиванием строки или кнопками «Переместить в самый верх», «Переместить выше», «Переместить ниже», «Переместить в самый низ».
- Правый щелчок по листу: «Дублировать» (лист напечатается ещё раз) и «Удалить».
- В разделе «Добавить листы из» можно выбрать любой открытый чертёж и добавить его листы по одному или все сразу. По умолчанию листы других чертежей в список не попадают.
- Внизу выбор: «Один файл PDF со всеми листами» или «Отдельный файл PDF для каждого листа».

**Панель «Слои»**
- Если слоёв много, у списка текущего слоя появляется полоса прокрутки.
- В панель можно добавить выпадающие списки «Текущий текстовый стиль», «Текущий размерный стиль» и «Текущий тип линий» (в «Настроить панель инструментов», раздел «Слои»). В новой настройке они уже стоят в конце панели. У длинных списков тоже есть прокрутка.

### English

**Application menu (the CADCraft button)**
- Taller rows, larger text, and a large icon to the left of each item.

**Icons**
- Save As: a large pencil beside the floppy disk.
- Import and Export: large arrows.

**TEXT and MLEADER**
- While you pick the text's start point, height and angle, a frame by the cursor shows where the line will go.
- Then the text editor opens right on the drawing, as for multiline text. Enter finishes the line and the editor opens for the next line below it. Enter on an empty line ends the command.
- While you pick a multileader's points, the segment to the cursor is shown. After the last point the text editor opens.
- The Properties palette offers leader type None for multileaders. The leader line and landing are hidden, not deleted: choosing Straight or Spline again brings them back. DXF files store it as an invisible leader.

**Block Manager**
- The Blocks button opens a Block Manager: the drawing's blocks in a list with search on the left, and on the right a preview of the selected block, its object and reference counts, base point and description.
- Buttons: Insert (or double-click a block), Edit Block, Rename, Delete (only a block nothing uses), Save to File (WBLOCK), Create.

**Plot Preview**
- Zoom In and Zoom Out buttons (a magnifier with plus and minus) and Zoom to Fit at the top. The mouse wheel zooms about the cursor.
- Drag the sheet to move it. Scroll bars appear at the bottom and on the right when the image is larger than the window.
- The window can be resized, and the image is rendered at a higher resolution.

**Batch Plot is now a print manager**
- Batch Plot opens its own window, listing the current drawing's sheets in tab order.
- Change the order by dragging a row or with Move to Top, Move Higher, Move Lower and Move to Bottom.
- Right-click a sheet for Duplicate (it prints again) and Delete.
- Under "Add sheets from" choose any open drawing and add its sheets one by one or all at once. Other drawings' sheets are not in the list unless you add them.
- At the bottom choose "One PDF file with all sheets" or "A separate PDF file for each sheet".

**Layers bar**
- The current-layer list scrolls when there are many layers.
- Current Text Style, Current Dimension Style and Current Linetype drop-downs can be added to the bar (Customize Tool Bar, Layers section). A fresh setup has them at the end of the bar. Long lists scroll too.

---

## 20.10.2026 (0.9.0-b10)

### Русский

**Шрифты SHX**
- Текст в стилях со шрифтами SHX (romans.shx, simplex.shx, txt.shx, ивритские шрифты вроде GIL.SHX и другие) теперь рисуется этими шрифтами, если файл .shx лежит в одной из папок шрифтов («Инструменты ▸ Параметры ▸ Шрифты»). Читаются обычные шрифты SHX, Unicode-шрифты и big fonts (BIGFONT).
- Если в шрифте нет нужного символа, он берётся из встроенного шрифта CADCraft.
- Строка с ивритом выкладывается справа налево, как в AutoCAD.
- Найденные шрифты SHX больше не попадают в сообщение «Не найдены шрифты».

**Штриховка: вкладка «Свои образцы»**
- В окне Hatch and Gradient («Штриховка и градиент») появилась третья вкладка «Свои образцы» (Custom Patterns). Укажите папку с файлами .pat (вручную или кнопкой «Обзор…»), и программа покажет все образцы из этих файлов списком.
- Рядом со списком видно, как выглядит выбранный образец.
- На вкладке есть угол, масштаб и исходная точка, как на вкладке «Шаблон». Справа те же разделы Boundaries и «Параметры».
- Папка запоминается. Кнопка «Обновить» перечитывает её после того, как вы добавили или изменили файлы.

**Листы (layouts)**
- Найдена причина, по которой листы из «אריה צוק» выглядели меньше, чем в AutoCAD. Чертёж в дюймовой системе (MEASUREMENT = 0), а листы настроены в миллиметрах. CADCraft брал единицы листа из настроек чертежа, и лист A3 получался 16,5 × 11,7 единицы вместо 420 × 297. Теперь единицы бумаги берутся из настроек самого листа, как в AutoCAD, и рамка помещается на лист так же.
- При сохранении эти единицы записываются в файл.

**Темы оформления**
- Добавлены темы «Чёрная» и «Светло-голубая» (#85abf6). Их можно выбрать в «Инструменты ▸ Параметры» или в меню «Вид ▸ Тема интерфейса».

**Ввод у курсора**
- Текст, который вы набираете в командной строке, теперь дублируется в окошке рядом с курсором: число сторон многоугольника, имя, параметр или имя команды. Окошко показывается и при выключенном динамическом вводе (F12).

### English

**SHX fonts**
- Text in styles that use SHX fonts (romans.shx, simplex.shx, txt.shx, Hebrew fonts such as GIL.SHX and others) is now drawn with those fonts when the .shx file is in one of the font folders (Tools ▸ Options ▸ Fonts). Regular SHX shape fonts, Unicode fonts and big fonts are read.
- Characters a font lacks are taken from CADCraft's built-in font.
- A line with Hebrew is laid out right to left, as in AutoCAD.
- SHX fonts that are found no longer appear in the "Fonts not found" list.

**Hatch: Custom Patterns tab**
- The Hatch and Gradient window has a third tab, Custom Patterns. Point it at a folder of .pat files (type the path or use Browse…) and it lists every pattern in those files.
- A preview of the selected pattern is shown beside the list.
- The tab has angle, scale and hatch origin, as the Pattern tab does. Boundaries and Options are on the right as before.
- The folder is remembered. Reload reads it again after you add or change files.

**Layouts**
- Found why the layouts of "אריה צוק" looked smaller than in AutoCAD. The drawing is in inches (MEASUREMENT = 0) but its layouts are set up in millimetres. CADCraft took the paper units from the drawing, so an A3 sheet came out 16.5 × 11.7 units instead of 420 × 297. Paper units now come from the layout's own settings, as in AutoCAD, and the title block fits the sheet the same way.
- These units are written back when the drawing is saved.

**Interface themes**
- Two new themes: Black and Light Blue (#85abf6). Choose them in Tools ▸ Options or View ▸ Interface Theme.

**Typed input by the cursor**
- What you type at the command line is now repeated in a small box next to the cursor: a polygon's number of sides, a name, an option or a command name. The box shows even with Dynamic Input (F12) off.

---

## 20.10.2026 (0.9.0-b9)

### Русский

**Палитра «Наборы инструментов» (Tool Sets)**
- Палитру можно превратить в ленту (ribbon), как в Microsoft Office. Лента встаёт прямо под строкой меню «Файл, Правка, Вид…».
- У ленты вкладки по задачам: «Рисование», «Модификация», «Аннотации», «Блок», «Параметризация», «3D-моделирование». На вкладке инструменты сгруппированы, название группы подписано снизу. Крупные кнопки подписаны, мелкие стоят столбиками по три.
- Если перетащить палитру к верхнему краю чертежа, она тоже становится лентой.
- Правый щелчок в любом месте палитры или ленты открывает меню: «Плавающая панель», «Закрепить слева», «Превратить в ленту», «Переместить в группу» (к слоям и свойствам или к другой группе панелей), «Закрыть панель». Пункта, который уже действует, в меню нет.
- Правый щелчок по кнопке инструмента показывает это же меню, а под ним прежние пункты раздела «Показывать на панели».

**Обрезать и Удлинить (TRIM / EXTEND)**
- Если перед запуском команды объекты уже выделены, они становятся режущими (граничными) кромками. Пока команда работает, они остаются подсвеченными. Подсветка снимается, когда команда завершается или отменяется.
- Щелчок в пустом месте начинает линию-«забор». Второй щелчок задаёт её конец, и всё, что эта линия пересекает, обрезается (удлиняется). Раньше в этом случае открывалась прямоугольная рамка. Рамка по-прежнему доступна через параметр Crossing в командной строке.

**Значки**
- «Переместить»: крупные стрелки.
- «Повернуть»: стрелка на конце дуги.
- «Обрезать»: ножницы, разрезающие линию надвое.
- «Удлинить»: крупная стрелка.
- «Параллельный» размер (Aligned): значок повёрнут на 13°.

### English

**The Tool Sets palette**
- The palette can turn into a ribbon, as in Microsoft Office. The ribbon sits right under the File, Edit, View… menu bar.
- The ribbon has a tab per task: Draw, Modify, Annotate, Block, Parametric, 3D Modeling. On each tab the tools are grouped, with the group's name underneath. Large buttons have labels, small ones are stacked in columns of three.
- Dragging the palette to the top edge of the drawing also turns it into the ribbon.
- Right-clicking anywhere in the palette or the ribbon opens a menu: Float Panel, Pin to Left, Transform to Ribbon, Move to Group (beside Layers/Properties or another panel group), Close Panel. The option already in effect is left out.
- Right-clicking a tool button shows the same menu, with the existing "Show on Tool Bar" items under it.

**TRIM / EXTEND**
- Objects selected before you start the command become its cutting (boundary) edges. They stay highlighted while the command runs, and the highlight goes away when the command ends or is cancelled.
- A click in empty space starts a fence line. A second click sets its end point, and everything the line crosses is trimmed (extended). Before this, a click in empty space opened a rectangular window. The window is still available through the Crossing option.

**Icons**
- Move: larger arrows.
- Rotate: an arrowhead at the end of the arc.
- Trim: scissors cutting a line in two.
- Extend: a larger arrow.
- Aligned dimension: the icon is turned 13°.

---

## 19.10.2026 (0.9.0-b8)

### Русский

**Размеры: ручка текста**
- «Растянуть» двигает размерную линию вместе с текстом поперёк. Текст при этом остаётся на своём месте относительно стрелок.
- «Переместить с размерной линией» ставит текст туда, где курсор, в том числе за стрелки. Размерная линия продлевается до текста.
- Пока вы тянете ручку, видно то же самое, что получится после щелчка.

**Мультивыноски в палитре «Свойства»**
- Можно менять стиль мультивыноски, тип выноски (прямая или сплайн), стрелку, размер стрелки, длину полки и высоту текста.
- Стрелку можно выбрать из того же списка, что у размеров, включая пользовательские блоки-стрелки чертежа.
- Стрелки мультивыносок из файлов DWG и DXF теперь читаются и сохраняются. Раньше все мультивыноски рисовались с замкнутой залитой стрелкой.

**Копирование между чертежами**
- Ctrl+C в одном открытом чертеже и Ctrl+V в другом теперь работают. Раньше после щелчка по вкладке другого чертежа программа не реагировала на Ctrl+V и на ввод команд.

**Блоки на листах**
- Двойной щелчок по блоку на листе (layout) открывает редактор блоков, как в пространстве модели.

**Имена файлов на иврите**
- На вкладках, в списке недавних файлов и в окне «Сохранить изменения?» имя читается так же, как в проводнике: буквы справа налево, «.dwg» справа.

### English

**Dimensions: the text grip**
- Stretch moves the dimension line across together with the text. The text keeps its place relative to the arrowheads.
- Move with Dim Line puts the text where the cursor is, even past the arrowheads. The dimension line is extended to reach the text.
- While you drag the grip, you see exactly what you will get when you click.

**Multileaders in the Properties palette**
- You can change the multileader style, the leader type (straight or spline), the arrowhead, the arrowhead size, the landing distance and the text height.
- The arrowhead list is the same one dimensions use, including the drawing's own arrowhead blocks.
- Multileader arrowheads are now read from and saved to DWG and DXF files. Before this, every multileader was drawn with a closed filled arrowhead.

**Copying between drawings**
- Ctrl+C in one open drawing and Ctrl+V in another now works. Before this, after clicking another drawing's tab, the program ignored Ctrl+V and typed commands.

**Blocks on layouts**
- Double-clicking a block on a layout opens the Block Editor, as it does in model space.

**Hebrew file names**
- On the drawing tabs, in the recent files list and in the "Save changes?" window, the name reads as it does in File Explorer: letters right to left, ".dwg" on the right.

---

## 19.10.2026 (0.9.0-b7)

### Русский

**Размеры**
- Пользовательские стрелки (блоки чертежа, например hatz или DIM1) рисуются правильно, с нужным размером и поворотом. Раньше они заменялись на «Замкнутую залитую».
- В «Свойствах» и в редакторе размерного стиля такая стрелка показывается как «Пользовательская: имя блока», и её можно выбрать из списка.
- «Переместить с выноской»: под текстом рисуется полочка, а от размерной линии к ней идёт выноска.
- Ручка текста стоит там, где текст виден на экране.
- Текст, который в AutoCAD сдвинут вдоль размерной линии, после открытия файла остаётся на месте и не прыгает в середину при первой правке.

**Блоки**
- Двойной щелчок по блоку открывает редактор блоков. Если у блока есть атрибуты, открывается их редактирование. Внешние ссылки двойной щелчок не трогает.

### English

**Dimensions**
- User arrowheads (blocks of the drawing, such as hatz or DIM1) are drawn correctly, at the right size and angle. Before this, they were replaced with "Closed filled".
- In Properties and in the dimension style editor such an arrowhead shows as "User Arrow: block name" and can be picked from the list.
- Move with Leader: a landing line is drawn under the text, with a leader from the dimension line to it.
- The text grip sits where the text appears on screen.
- Text that was moved along the dimension line in AutoCAD stays in place after the file is opened, and no longer jumps to the middle on the first edit.

**Blocks**
- Double-clicking a block opens the Block Editor. If the block has attributes, their editor opens instead. External references are left alone.

---

## 19.10.2026

### Русский

**Размерные стили**
- Новый диспетчер размерных стилей: создание, изменение, сравнение стилей и выбор текущего.
- Редактор стиля с вкладками: «Линии», «Символы и стрелки», «Текст», «Размещение», «Основные единицы», «Альтернативные единицы», «Допуски».
- В «Свойствах» у выбранного размера видны настройки его стиля (стрелки, линии, текст и другие). Изменение настройки применяется только к этому размеру, как переопределение в AutoCAD.
- У ручки текста размера появилось меню: «Переместить с размерной линией», «Переместить только текст», «Переместить с выноской», «Над размерной линией», «По центру по вертикали» и «Восстановить положение текста».

**Интерфейс**
- Кнопка приложения (меню «Файл») стоит первой на панели инструментов. В меню есть пакетная печать.
- Окно «Параметры» больше не меняет размер при переходе между вкладками.

### English

**Dimension styles**
- New Dimension Style Manager: create, modify and compare styles, and set the current one.
- Style editor with tabs: Lines, Symbols and Arrows, Text, Fit, Primary Units, Alternate Units, Tolerances.
- In Properties, a selected dimension shows its style settings (arrowheads, lines, text and more). Changing a setting changes only that dimension, like an override in AutoCAD.
- The dimension text grip has a menu: Move with Dim Line, Move Text Only, Move with Leader, Above Dim Line, Center Vertically and Reset Text Position.

**Interface**
- The application button (the File menu) is the first button on the Tool Bar. Its menu includes Batch Print.
- The Options window keeps the same size on every tab.

---

## 18.10.2026

### Русский

**Панели инструментов**
- Панель инструментов можно закрепить сверху или слева, перетащив её за маркер.
- Кнопки можно переставлять перетаскиванием, добавлять и убирать в окне «Настроить панель инструментов». Там же можно вернуть всё по умолчанию.
- Подсказки у кнопок показывают название команды, её псевдоним и сочетание клавиш.
- Отдельные панели «Наборы инструментов» и «Слои» тоже закрепляются сверху или слева и настраиваются. На панели «Слои» есть выпадающий список текущего слоя.

**Шрифты**
- В «Инструменты ▸ Параметры ▸ Шрифты» можно добавить свои папки со шрифтами (.ttf, .otf, .ttc). Они просматриваются раньше системных шрифтов.
- После открытия чертежа программа сообщает, каких шрифтов не хватает и каким шрифтом рисуется такой текст.

**Редактирование текста на месте**
- Двойной щелчок по тексту, многострочному тексту, размеру, мультивыноске или ячейке таблицы открывает редактор прямо на чертеже.
- В ячейках таблицы Tab переходит к следующей ячейке, Shift+Tab к предыдущей, Alt+↑/↓ к ячейке выше или ниже.
- В тексте размера можно вставить измеренное значение (<>) или вернуть только измеренное значение.

**Редактор текста**
- Панель редактора нарисована заново, кнопки стоят в два ряда. Есть кнопка закрытия.
- Добавлено меню «Дополнительные символы»: ≈, ≠, ≤, ≥, ×, ÷, √, ∞, Δ, Ω, µ, π, α, β, γ, φ, ², ³, ½, ¼, ¾, ‰ и другие.
- Если в шрифте текста нет нужного символа, он берётся из запасного шрифта, а не пропадает.

### English

**Tool bars**
- The Tool Bar can be docked at the top or on the left by dragging its handle.
- Buttons can be reordered by dragging, and added or removed in the Customize Tool Bar window. You can also reset everything to the defaults there.
- Button tooltips show the command name, its alias and its keyboard shortcut.
- The separate Tool Sets and Layers tool bars can also be docked at the top or on the left, and customized. The Layers bar has a current layer drop-down.

**Fonts**
- In Tools ▸ Options ▸ Fonts you can add your own font folders (.ttf, .otf, .ttc). They are searched before the system fonts.
- After a drawing opens, the program reports which fonts are missing and which font that text is drawn with instead.

**Editing text in place**
- Double-clicking text, multiline text, a dimension, a multileader or a table cell opens the editor right on the drawing.
- In table cells, Tab moves to the next cell, Shift+Tab to the previous one, Alt+↑/↓ to the cell above or below.
- In dimension text you can insert the measured value (<>) or go back to the measured value only.

**Text editor**
- The editor's toolbar has been redrawn, with buttons in two rows. There is a close button.
- New Additional symbols menu: ≈, ≠, ≤, ≥, ×, ÷, √, ∞, Δ, Ω, µ, π, α, β, γ, φ, ², ³, ½, ¼, ¾, ‰ and more.
- When the text's font has no glyph for a symbol, it is taken from a fallback font instead of disappearing.
