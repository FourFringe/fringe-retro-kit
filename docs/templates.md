# Character Templates

A **template** is a named, pre-baked set of field values you can apply to an existing
character — for example a favourite Ultima II fighter build, or a "top up my food and gold"
tweak. Applying a template is exactly the same as editing those fields by hand: the values
are validated and changed **in memory**, and nothing is written to disk until you save
(press `s` in the editor). That means you can apply several templates, make manual edits,
and then save once.

Templates only ever modify the fields they list; every other field on the character is left
untouched. There is no "new character from a template" feature — you always start from a
save the game itself created.

## Where templates live

Templates are read from `templates.toml` in the current directory, or from the path in the
`FRINGE_RETRO_TEMPLATES` environment variable. A missing file simply means "no templates".
Copy [`templates.example.toml`](../templates.example.toml) to `templates.toml` to get
started.

## File format

The file is a list of `[[template]]` tables:

```toml
[[template]]
game = "ultima2"
name = "Fighter"
description = "A melee bruiser: high strength/stamina, sword and plate armour."
fields = { strength = 35, stamina = 40, weapon = "Sword", armor = "Plate", food = 3000 }
```

| Key | Required | Meaning |
| --- | --- | --- |
| `game` | yes | A known game id: `ultima1`, `ultima2`, or `ultima3`. |
| `name` | yes | A short label shown in the picker and in `fringe-retro templates`. |
| `description` | no | A longer note shown in the template preview. |
| `fields` | yes | A map of **field key → value** to apply. |

### Value syntax

Values use the same syntax as the interactive editor:

- **Numbers** — written as bare numbers: `gold = 500`.
- **Enum fields** — the value **name** in quotes (case-insensitive), e.g. `weapon = "Sword"`,
  or its underlying number: `weapon = 5`.
- **Letter fields** (some Ultima III fields) — the value name, e.g. `class = "Paladin"`.
- **Boolean fields** — `true`/`false` (also `yes`/`no`).

## Validation

Templates are checked before they can be applied. When you open the template picker (or run
`fringe-retro templates`), each template is dry-run against the target game's field schema.
A template that references an **unknown field**, an **out-of-range number**, or an **invalid
enum name** is shown with a `✗` marker and cannot be applied; its error is shown in the
preview. Valid templates apply normally.

List your templates and their validity from the command line:

```bash
fringe-retro templates
```

```
ultima1    Starter boost            3 field(s)  [ok]
ultima2    Fighter                  5 field(s)  [ok]
ultima2    Top up resources         2 field(s)  [ok]
ultima3    Tank                     4 field(s)  [ok]
```

## Applying a template (interactive UI)

1. Launch `fringe-retro`, pick a game, and open a character in the editor.
2. Press `t` to open the template picker. Only templates for that game are shown.
3. Use `↑`/`↓` to select; the preview on the right lists the fields it will set (and any
   validation error).
4. Press `Enter` (or `a`) to apply the selected template. The character's fields update and
   the session is marked unsaved (`●`).
5. Apply more templates or edit fields as you like, then press `s` to save.

## Capturing a template (interactive UI)

Rather than hand-writing a template, you can capture one from a character you've set up:

1. In the editor, arrange the character the way you want (edit fields, or apply other
   templates).
2. Press `T` (capital). The field list gains checkboxes; any fields you changed this session
   are **pre-checked**.
3. `Space` toggles the selected field, `a` toggles all. Pick the fields the template should
   set.
4. Press `Enter`, type a name, and press `Enter` again.

The template is **appended** to `templates.toml` — existing templates and comments are left
untouched — and becomes available in the picker straight away. Captured numeric values are
written as plain numbers and enum/letter values as their names (e.g. `weapon = "Sword"`).
Descriptions aren't captured; add one by editing the file if you like.

---

## Field reference

The tables below list the field keys you can use in a template's `fields` map for each game,
along with the accepted values. These are the same fields shown in the editor. Ranges are
inclusive.

Ultima II and Ultima III store most numbers as BCD, which limits their range to decimal
digits (a 1-byte field is `0–99`, a 2-byte field is `0–9999`).

### Ultima I (`ultima1`)

**Character:** `name` (text) · `race` · `class` · `sex`

| Enum field | Allowed values |
| --- | --- |
| `race` | Human, Elf, Dwarf, Bobbit |
| `class` | Fighter, Cleric, Wizard, Thief |
| `sex` | Male, Female |
| `weapon` (ready) | None, Dagger, Mace, Axe, Rope & Spikes, Sword, Great Sword, Bow & Arrows, Amulet, Wand, Staff, Triangle, Pistol, Light Sword, Phazor, Blaster |
| `spell` (ready) | None, Open, Unlock, Magic Missile, Steal, Ladder Down, Ladder Up, Blink, Create, Destroy, Kill |
| `armour` (ready) | None, Leather, Chain Mail, Plate Mail, Vacuum Suit, Reflect Suit |
| `transport` | Walking, Horse, Cart, Raft, Frigate, Aircar |

**Attributes (0–9999):** `strength` `agility` `stamina` `charisma` `wisdom` `intelligence`

**Status (0–9999):** `hits` `gold` `experience` `food`

**Location (0–65535):** `x` `y` `last_signpost` `steps`

**Inventory counts (0–9999):**
- Gems: `gem_red` `gem_green` `gem_blue` `gem_white`
- Armour: `armour_leather` `armour_chain_mail` `armour_plate_mail` `armour_vacuum_suit` `armour_reflect_suit`
- Weapons: `weapon_dagger` `weapon_mace` `weapon_axe` `weapon_rope_spikes` `weapon_sword` `weapon_great_sword` `weapon_bow` `weapon_amulet` `weapon_wand` `weapon_staff` `weapon_triangle` `weapon_pistol` `weapon_light_sword` `weapon_phazor` `weapon_blaster`
- Spells: `spell_open` `spell_unlock` `spell_magic_missile` `spell_steal` `spell_ladder_down` `spell_ladder_up` `spell_blink` `spell_create` `spell_destroy` `spell_kill`
- Transports: `transport_horse` `transport_cart` `transport_raft` `transport_frigate` `transport_aircar` `transport_shuttle` `transport_time_machine`

### Ultima II (`ultima2`)

**Character:** `name` (text) · `sex` · `class` · `race`

| Enum field | Allowed values |
| --- | --- |
| `sex` | Male, Female |
| `class` | Fighter, Cleric, Wizard, Thief |
| `race` | Human, Elf, Dwarf, Hobbit |
| `weapon` (readied) | None, Dagger, Mace, Axe, Bow, Sword, Great sword, Light sword, Phaser, Quicksword |
| `armor` (worn) | None, Cloth, Leather, Chain, Plate, Reflect, Power |

**Attributes (0–99):** `strength` `agility` `stamina` `charisma` `wisdom` `intelligence`

**Status (0–9999):** `hits` `food` `experience` `gold`

**Map (0–255):** `x` `y`

**Inventory counts (0–99, tentative mapping):**
- Weapons: `weapon_dagger` `weapon_mace` `weapon_axe` `weapon_bow` `weapon_sword` `weapon_greatsword` `weapon_lightsword` `weapon_phaser` `weapon_quicksword`
- Armour: `armor_cloth` `armor_leather` `armor_chain` `armor_plate` `armor_reflect` `armor_power`

### Ultima III (`ultima3`)

Templates apply to a single character (a roster slot or party member).

**Character:** `name` (text) · `race` · `class` · `gender` · `status` · `in_party`

| Field | Allowed values |
| --- | --- |
| `race` | Human, Elf, Dwarf, Fuzzy, Bobbit |
| `class` | Fighter, Cleric, Wizard, Thief, Paladin, Barbarian, Lark, Illusionist, Alchemist, Druid, Ranger |
| `gender` | Male, Female, Other |
| `status` | Good, Poisoned, Dead, Ashes |
| `in_party` | true / false |
| `worn_armor` | index number (0–255) |
| `weapon` (ready) | index number (0–255) |
| `marks_cards` | bitfield: Love, Sol, Moon, Death, Force, Fire, Snake, Kings |

**Attributes (0–99):** `strength` `dexterity` `intelligence` `wisdom`

**Vitals:** `magic` (0–99) · `hits` (0–9999) · `max_hits` (0–9999) · `experience` (0–9999)

**Resources:** `food` (0–9999) · `food_frac` (0–99) · `gold` (0–9999) · `gems` (0–99) · `keys` (0–99) · `powders` (0–99) · `torches` (0–99)

**Armour counts (0–99):** `armor_cloth` `armor_leather` `armor_chain` `armor_plate` `armor_chain_plus2` `armor_plate_plus2` `armor_exotic`

**Weapon counts (0–99):** `weapon_dagger` `weapon_mace` `weapon_sling` `weapon_axe` `weapon_bow` `weapon_sword` `weapon_2h_sword` `weapon_axe_plus2` `weapon_bow_plus2` `weapon_sword_plus2` `weapon_gloves` `weapon_axe_plus4` `weapon_bow_plus4` `weapon_sword_plus4` `weapon_exotic`
