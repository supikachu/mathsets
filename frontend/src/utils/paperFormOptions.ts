/** 试卷信息表单：学段/年级、省/市级联字典（与题库侧栏对齐并补全市区） */

export const YEAR_OPTIONS = ['2020', '2021', '2022', '2023', '2024', '2025', '2026']

export const STAGE_OPTIONS = [
  { label: '初中', value: 'junior' },
  { label: '高中', value: 'senior' },
] as const

export const SUBJECT_OPTIONS = [
  { label: '数学', value: '数学' },
  { label: '物理', value: '物理' },
] as const

export const SEMESTER_OPTIONS = [
  { label: '上学期', value: 'first' },
  { label: '下学期', value: 'second' },
  { label: '全年', value: 'full_year' },
] as const

export function gradesForStage(stage?: string | null): { label: string; value: string }[] {
  if (stage === 'junior') {
    return [
      { label: '七年级', value: '七年级' },
      { label: '八年级', value: '八年级' },
      { label: '九年级', value: '九年级' },
    ]
  }
  if (stage === 'senior') {
    return [
      { label: '高一', value: '高一' },
      { label: '高二', value: '高二' },
      { label: '高三', value: '高三' },
    ]
  }
  return []
}

/** 试卷筛选：年级+学期组合（高一上学期 → grade=高一, semester=first） */
export function gradeSemesterOptions(stage?: string | null): { label: string; grade: string; semester: string }[] {
  const grades = gradesForStage(stage)
  const out: { label: string; grade: string; semester: string }[] = []
  for (const g of grades) {
    out.push({ label: `${g.label}上学期`, grade: g.value, semester: 'first' })
    out.push({ label: `${g.label}下学期`, grade: g.value, semester: 'second' })
  }
  return out
}

export const CITY_OPTIONS: Record<string, string[]> = {
  北京: ['东城区', '西城区', '朝阳区', '海淀区', '丰台区', '石景山区', '通州区', '昌平区', '大兴区'],
  上海: ['黄浦区', '徐汇区', '静安区', '浦东新区', '杨浦区', '闵行区', '宝山区', '嘉定区', '松江区'],
  天津: ['和平区', '河西区', '南开区', '河东区', '河北区', '滨海新区'],
  重庆: ['渝中区', '江北区', '南岸区', '沙坪坝区', '九龙坡区', '渝北区'],
  浙江: ['杭州市', '宁波市', '温州市', '绍兴市', '嘉兴市', '湖州市', '金华市', '台州市', '舟山市', '衢州市', '丽水市'],
  江苏: ['南京市', '苏州市', '无锡市', '常州市', '南通市', '扬州市', '徐州市', '镇江市', '泰州市', '盐城市', '淮安市'],
  广东: ['广州市', '深圳市', '珠海市', '佛山市', '东莞市', '中山市', '惠州市', '汕头市'],
  湖北: ['武汉市', '宜昌市', '襄阳市', '荆州市', '黄冈市'],
  湖南: ['长沙市', '株洲市', '湘潭市', '衡阳市', '岳阳市'],
  四川: ['成都市', '绵阳市', '德阳市', '宜宾市', '南充市'],
  山东: ['济南市', '青岛市', '烟台市', '潍坊市', '临沂市'],
  福建: ['福州市', '厦门市', '泉州市', '漳州市', '莆田市'],
  安徽: ['合肥市', '芜湖市', '蚌埠市', '阜阳市', '安庆市'],
  河南: ['郑州市', '洛阳市', '开封市', '南阳市', '新乡市'],
  陕西: ['西安市', '宝鸡市', '咸阳市', '渭南市', '延安市'],
  江西: ['南昌市', '九江市', '赣州市', '上饶市', '宜春市'],
  辽宁: ['沈阳市', '大连市', '鞍山市', '锦州市'],
  河北: ['石家庄市', '唐山市', '保定市', '邯郸市', '廊坊市'],
}

export const PROVINCE_OPTIONS = Object.keys(CITY_OPTIONS)

/** 「浙江省」→「浙江」，「北京市」→「北京」 */
export function canonicalProvince(raw?: string | null): string {
  if (!raw) return ''
  const s = raw.trim()
  if (!s) return ''
  const hit = PROVINCE_OPTIONS.find((p) => s === p || s.startsWith(p))
  if (hit) return hit
  return s.replace(/省$/, '').replace(/市$/, '')
}

export function citiesForProvince(province?: string | null): string[] {
  const key = canonicalProvince(province)
  return CITY_OPTIONS[key] ?? []
}
