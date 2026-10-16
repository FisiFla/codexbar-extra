import Foundation

// MARK: - Amount Response Models

struct DeepSeekAmountPayload: Decodable {
    let code: Int?
    let msg: String?
    let data: DeepSeekAmountData?

    private enum CodingKeys: String, CodingKey {
        case code, msg, data
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        self.code = try container.decodeIfPresent(Int.self, forKey: .code)
        self.msg = try container.decodeIfPresent(String.self, forKey: .msg)
        if let code, code != 0 {
            // Error envelopes are not schema-stable. Preserve their code even if `data` has an unexpected shape.
            self.data = try? container.decodeIfPresent(DeepSeekAmountData.self, forKey: .data)
        } else {
            self.data = try container.decodeIfPresent(DeepSeekAmountData.self, forKey: .data)
        }
    }
}

struct DeepSeekAmountData: Decodable {
    let bizCode: Int?
    let bizMsg: String?
    let bizData: DeepSeekAmountBizData?

    private enum CodingKeys: String, CodingKey {
        case bizCode = "biz_code"
        case bizMsg = "biz_msg"
        case bizData = "biz_data"
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        self.bizCode = try container.decodeIfPresent(Int.self, forKey: .bizCode)
        self.bizMsg = try container.decodeIfPresent(String.self, forKey: .bizMsg)
        if let bizCode, bizCode != 0 {
            self.bizData = try? container.decodeIfPresent(DeepSeekAmountBizData.self, forKey: .bizData)
        } else {
            self.bizData = try container.decodeIfPresent(DeepSeekAmountBizData.self, forKey: .bizData)
        }
    }
}

struct DeepSeekAmountBizData: Decodable {
    let total: [DeepSeekModelUsage]?
    let days: [DeepSeekDayUsage]?

    private enum CodingKeys: String, CodingKey {
        case total, days
    }
}

// MARK: - Cost Response Models

struct DeepSeekCostPayload: Decodable {
    let code: Int?
    let msg: String?
    let data: DeepSeekCostData?

    private enum CodingKeys: String, CodingKey {
        case code, msg, data
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        self.code = try container.decodeIfPresent(Int.self, forKey: .code)
        self.msg = try container.decodeIfPresent(String.self, forKey: .msg)
        if let code, code != 0 {
            self.data = try? container.decodeIfPresent(DeepSeekCostData.self, forKey: .data)
        } else {
            self.data = try container.decodeIfPresent(DeepSeekCostData.self, forKey: .data)
        }
    }
}

struct DeepSeekCostData: Decodable {
    let bizCode: Int?
    let bizMsg: String?
    let bizData: [DeepSeekCostBizDataItem]?

    private enum CodingKeys: String, CodingKey {
        case bizCode = "biz_code"
        case bizMsg = "biz_msg"
        case bizData = "biz_data"
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        self.bizCode = try container.decodeIfPresent(Int.self, forKey: .bizCode)
        self.bizMsg = try container.decodeIfPresent(String.self, forKey: .bizMsg)
        if let bizCode, bizCode != 0 {
            self.bizData = try? container.decodeIfPresent([DeepSeekCostBizDataItem].self, forKey: .bizData)
        } else {
            self.bizData = try container.decodeIfPresent([DeepSeekCostBizDataItem].self, forKey: .bizData)
        }
    }
}

struct DeepSeekCostBizDataItem: Decodable {
    let total: [DeepSeekCostModelUsage]?
    let days: [DeepSeekCostDayUsage]?
    let currency: String?

    private enum CodingKeys: String, CodingKey {
        case total, days, currency
    }
}

// MARK: - Shared Models

struct DeepSeekModelUsage: Decodable {
    let model: String?
    let usage: [DeepSeekUsageItem]?

    private enum CodingKeys: String, CodingKey {
        case model, usage
    }
}

struct DeepSeekDayUsage: Decodable {
    let date: String?
    let data: [DeepSeekModelUsage]?

    private enum CodingKeys: String, CodingKey {
        case date, data
    }
}

struct DeepSeekUsageItem: Decodable {
    let type: String?
    let amount: String?

    private enum CodingKeys: String, CodingKey {
        case type, amount
    }
}

struct DeepSeekCostModelUsage: Decodable {
    let model: String?
    let usage: [DeepSeekCostItem]?

    private enum CodingKeys: String, CodingKey {
        case model, usage
    }
}

struct DeepSeekCostDayUsage: Decodable {
    let date: String?
    let data: [DeepSeekCostModelUsage]?

    private enum CodingKeys: String, CodingKey {
        case date, data
    }
}

struct DeepSeekCostItem: Decodable {
    let type: String?
    let amount: String?

    private enum CodingKeys: String, CodingKey {
        case type, amount
    }
}

// MARK: - Domain Models

public struct DeepSeekUsageSummary: Sendable, Equatable {
    public let todayTokens: Int
    public let currentMonthTokens: Int
    public let todayCost: Double?
    public let currentMonthCost: Double?
    public let requestCount: Int
    public let currentMonthRequestCount: Int
    public let topModel: String?
    public let categoryBreakdown: [DeepSeekCategoryBreakdown]
    public let daily: [DeepSeekDailyUsage]
    public let modelCosts: [DeepSeekModelCost]
    public let currency: String
    public let apiKeyCount: Int
    public let period: DeepSeekUsagePeriod
    public let updatedAt: Date

    public init(
        todayTokens: Int,
        currentMonthTokens: Int,
        todayCost: Double?,
        currentMonthCost: Double?,
        requestCount: Int,
        currentMonthRequestCount: Int,
        topModel: String?,
        categoryBreakdown: [DeepSeekCategoryBreakdown],
        daily: [DeepSeekDailyUsage],
        currency: String,
        modelCosts: [DeepSeekModelCost] = [],
        apiKeyCount: Int = 0,
        period: DeepSeekUsagePeriod = .currentMonth,
        updatedAt: Date)
    {
        self.todayTokens = todayTokens
        self.currentMonthTokens = currentMonthTokens
        self.todayCost = todayCost
        self.currentMonthCost = currentMonthCost
        self.requestCount = requestCount
        self.currentMonthRequestCount = currentMonthRequestCount
        self.topModel = topModel
        self.categoryBreakdown = categoryBreakdown
        self.daily = daily
        self.modelCosts = modelCosts
        self.currency = currency
        self.apiKeyCount = apiKeyCount
        self.period = period
        self.updatedAt = updatedAt
    }
}

public struct DeepSeekModelCost: Sendable, Equatable {
    public let model: String
    public let cost: Double

    public init(model: String, cost: Double) {
        self.model = model
        self.cost = cost
    }
}

public enum DeepSeekUsagePeriod: Sendable, Equatable {
    case last30Days
    case currentMonth
}

public struct DeepSeekCategoryBreakdown: Sendable, Equatable {
    public let category: DeepSeekUsageCategory
    public let tokens: Int
    public let cost: Double?

    public init(category: DeepSeekUsageCategory, tokens: Int, cost: Double?) {
        self.category = category
        self.tokens = tokens
        self.cost = cost
    }
}

public enum DeepSeekUsageCategory: String, Sendable, Equatable {
    case promptCacheHitToken = "PROMPT_CACHE_HIT_TOKEN"
    case promptCacheMissToken = "PROMPT_CACHE_MISS_TOKEN"
    case responseToken = "RESPONSE_TOKEN"
    case request = "REQUEST"

    public init?(rawValue: String) {
        switch rawValue.uppercased() {
        case "PROMPT_CACHE_HIT_TOKEN":
            self = .promptCacheHitToken
        case "PROMPT_CACHE_MISS_TOKEN":
            self = .promptCacheMissToken
        case "RESPONSE_TOKEN":
            self = .responseToken
        case "REQUEST":
            self = .request
        default:
            return nil
        }
    }
}

public struct DeepSeekDailyUsage: Sendable, Equatable {
    public let date: String
    public let totalTokens: Int
    public let cost: Double?
    public let requestCount: Int
    public let inputTokens: Int?
    public let outputTokens: Int?
    public let cacheReadTokens: Int?
    public let modelBreakdowns: [CostUsageDailyReport.ModelBreakdown]?

    public init(
        date: String,
        totalTokens: Int,
        cost: Double?,
        requestCount: Int,
        inputTokens: Int? = nil,
        outputTokens: Int? = nil,
        cacheReadTokens: Int? = nil,
        modelBreakdowns: [CostUsageDailyReport.ModelBreakdown]? = nil)
    {
        self.date = date
        self.totalTokens = totalTokens
        self.cost = cost
        self.requestCount = requestCount
        self.inputTokens = inputTokens
        self.outputTokens = outputTokens
        self.cacheReadTokens = cacheReadTokens
        self.modelBreakdowns = modelBreakdowns
    }

    public var modelsUsed: [String]? {
        self.modelBreakdowns?.map(\.modelName)
    }
}

// MARK: - Parsing

enum DeepSeekUsageCostParser {
    static func decodeAmountPayload(data: Data) throws -> DeepSeekAmountPayload {
        try JSONDecoder().decode(DeepSeekAmountPayload.self, from: data)
    }

    static func decodeCostPayload(data: Data) throws -> DeepSeekCostPayload {
        try JSONDecoder().decode(DeepSeekCostPayload.self, from: data)
    }

    static func parse(
        amountData: Data,
        costData: Data,
        now: Date = Date(),
        calendar: Calendar = .current) throws -> DeepSeekUsageSummary
    {
        let amountPayload: DeepSeekAmountPayload
        let costPayload: DeepSeekCostPayload
        do {
            amountPayload = try self.decodeAmountPayload(data: amountData)
        } catch {
            throw DeepSeekUsageError.parseFailed("amount: \(self.decodingFailureDescription(error))")
        }
        do {
            costPayload = try self.decodeCostPayload(data: costData)
        } catch {
            throw DeepSeekUsageError.parseFailed("cost: \(self.decodingFailureDescription(error))")
        }

        // Validate responses
        if let code = amountPayload.code, code != 0 {
            if self.isAuthenticationError(code) {
                throw DeepSeekUsageError.invalidPlatformToken
            }
            throw DeepSeekUsageError.apiError("amount code \(code)")
        }
        if let bizCode = amountPayload.data?.bizCode, bizCode != 0 {
            if self.isAuthenticationError(bizCode) {
                throw DeepSeekUsageError.invalidPlatformToken
            }
            throw DeepSeekUsageError.apiError("amount biz_code \(bizCode)")
        }
        if let code = costPayload.code, code != 0 {
            if self.isAuthenticationError(code) {
                throw DeepSeekUsageError.invalidPlatformToken
            }
            throw DeepSeekUsageError.apiError("cost code \(code)")
        }
        if let bizCode = costPayload.data?.bizCode, bizCode != 0 {
            if self.isAuthenticationError(bizCode) {
                throw DeepSeekUsageError.invalidPlatformToken
            }
            throw DeepSeekUsageError.apiError("cost biz_code \(bizCode)")
        }

        guard let amountBizData = amountPayload.data?.bizData else {
            throw DeepSeekUsageError.parseFailed("Missing amount biz_data")
        }

        let currency = costPayload.data?.bizData?.first?.currency ?? "CNY"

        // Parse total amounts
        let totalAmounts = amountBizData.total ?? []
        let totalCosts = costPayload.data?.bizData?.first?.total ?? []

        // Parse daily data
        let dailyAmounts = amountBizData.days ?? []
        let dailyCosts = costPayload.data?.bizData?.first?.days ?? []

        return self.aggregate(input: AggregationInput(
            totalAmounts: totalAmounts,
            totalCosts: totalCosts,
            dailyAmounts: dailyAmounts,
            dailyCosts: dailyCosts,
            currency: currency,
            now: now,
            calendar: calendar))
    }

    fileprivate static func isAuthenticationError(_ code: Int) -> Bool {
        code == 40002 || code == 40003
    }

    fileprivate static func decodingFailureDescription(_ error: any Error) -> String {
        if error is DecodingError {
            return String(describing: error)
        }
        return error.localizedDescription
    }

    // MARK: - Aggregation

    fileprivate struct AggregationContext {
        let calendar: Calendar
        let todayString: String
        let startOfMonth: Date
        let now: Date
        let dailyAmountMap: [String: [String: [DeepSeekUsageItem]]]
        let dailyCostMap: [String: [String: [DeepSeekCostItem]]]
        let allDates: Set<String>

        init(
            dailyAmounts: [DeepSeekDayUsage],
            dailyCosts: [DeepSeekCostDayUsage],
            now: Date,
            calendar: Calendar)
        {
            self.calendar = calendar
            self.now = now
            self.todayString = Self.dayString(now, calendar: calendar)

            var components = calendar.dateComponents([.year, .month], from: now)
            components.day = 1
            self.startOfMonth = calendar.date(from: components) ?? now

            self.dailyAmountMap = Self.buildAmountMap(from: dailyAmounts)
            self.dailyCostMap = Self.buildCostMap(from: dailyCosts)

            var dates: Set<String> = []
            for date in self.dailyAmountMap.keys {
                dates.insert(date)
            }
            for date in self.dailyCostMap.keys {
                dates.insert(date)
            }
            self.allDates = dates
        }

        static func dayString(_ date: Date, calendar: Calendar) -> String {
            let components = calendar.dateComponents([.year, .month, .day], from: date)
            guard let year = components.year,
                  let month = components.month,
                  let day = components.day
            else { return "" }
            return CostUsageLocalDay.key(year: year, month: month, day: day)
        }

        static func buildAmountMap(
            from dailyAmounts: [DeepSeekDayUsage]) -> [String: [String: [DeepSeekUsageItem]]]
        {
            var result: [String: [String: [DeepSeekUsageItem]]] = [:]
            for dayUsage in dailyAmounts {
                guard let date = dayUsage.date else { continue }
                var modelMap = result[date] ?? [:]
                for modelUsage in dayUsage.data ?? [] {
                    guard let model = modelUsage.model else { continue }
                    let items = modelUsage.usage ?? []
                    if !items.isEmpty {
                        modelMap[model, default: []].append(contentsOf: items)
                    }
                }
                if !modelMap.isEmpty {
                    result[date] = modelMap
                }
            }
            return result
        }

        static func buildCostMap(
            from dailyCosts: [DeepSeekCostDayUsage]) -> [String: [String: [DeepSeekCostItem]]]
        {
            var result: [String: [String: [DeepSeekCostItem]]] = [:]
            for dayUsage in dailyCosts {
                guard let date = dayUsage.date else { continue }
                var modelMap = result[date] ?? [:]
                for modelUsage in dayUsage.data ?? [] {
                    guard let model = modelUsage.model else { continue }
                    let items = modelUsage.usage ?? []
                    if !items.isEmpty {
                        modelMap[model, default: []].append(contentsOf: items)
                    }
                }
                if !modelMap.isEmpty {
                    result[date] = modelMap
                }
            }
            return result
        }
    }

    private struct AggregationInput {
        let totalAmounts: [DeepSeekModelUsage]
        let totalCosts: [DeepSeekCostModelUsage]
        let dailyAmounts: [DeepSeekDayUsage]
        let dailyCosts: [DeepSeekCostDayUsage]
        let currency: String
        let now: Date
        let calendar: Calendar
    }

    private static func aggregate(input: AggregationInput) -> DeepSeekUsageSummary {
        let ctx = AggregationContext(
            dailyAmounts: input.dailyAmounts,
            dailyCosts: input.dailyCosts,
            now: input.now,
            calendar: input.calendar)

        // Today aggregation
        let todayResult = self.aggregateDay(
            dateString: ctx.todayString,
            amountMap: ctx.dailyAmountMap,
            costMap: ctx.dailyCostMap,
            calendar: ctx.calendar)

        // Month aggregation
        let dailyCtx = DailyAggregationContext(
            allDates: ctx.allDates,
            startOfMonth: ctx.startOfMonth,
            now: ctx.now,
            amountMap: ctx.dailyAmountMap,
            costMap: ctx.dailyCostMap,
            calendar: ctx.calendar)
        let monthResult = self.aggregateMonth(ctx: dailyCtx)

        // Model and category breakdown from totals
        let (topModel, categoryBreakdown, modelCosts) = self.buildBreakdowns(
            totalAmounts: input.totalAmounts,
            totalCosts: input.totalCosts)

        // Daily usage array
        let dailyUsages = self.buildDailyUsages(ctx: dailyCtx)

        return DeepSeekUsageSummary(
            todayTokens: todayResult.tokens,
            currentMonthTokens: monthResult.tokens,
            todayCost: todayResult.cost,
            currentMonthCost: monthResult.cost,
            requestCount: todayResult.requests,
            currentMonthRequestCount: monthResult.requests,
            topModel: topModel,
            categoryBreakdown: categoryBreakdown,
            daily: dailyUsages,
            currency: input.currency,
            modelCosts: modelCosts,
            apiKeyCount: 0,
            period: .currentMonth,
            updatedAt: input.now)
    }

    private struct DayAggregationResult {
        let tokens: Int
        let cost: Double?
        let requests: Int
    }

    private struct DailyAggregationContext {
        let allDates: Set<String>
        let startOfMonth: Date
        let now: Date
        let amountMap: [String: [String: [DeepSeekUsageItem]]]
        let costMap: [String: [String: [DeepSeekCostItem]]]
        let calendar: Calendar
    }

    private static func aggregateDay(
        dateString: String,
        amountMap: [String: [String: [DeepSeekUsageItem]]],
        costMap: [String: [String: [DeepSeekCostItem]]],
        calendar: Calendar) -> DayAggregationResult
    {
        var tokens = 0
        var cost: Double?
        var requests = 0
        var dayCostUnavailable = false

        if let amounts = amountMap[dateString] {
            for items in amounts.values {
                for item in items {
                    guard let category = DeepSeekUsageCategory(rawValue: item.type ?? "") else { continue }
                    if category == .request {
                        requests += self.parseTokenAmount(item.amount)
                    } else {
                        tokens += self.parseTokenAmount(item.amount)
                    }
                }
            }
        }

        if let costs = costMap[dateString] {
            for items in costs.values {
                for item in items {
                    guard let category = DeepSeekUsageCategory(rawValue: item.type ?? "") else {
                        dayCostUnavailable = true
                        continue
                    }
                    if category != .request {
                        guard let amount = Self.parseValidCostAmount(item.amount) else {
                            dayCostUnavailable = true
                            continue
                        }
                        if let existing = cost {
                            let total = existing + amount
                            if total.isFinite {
                                cost = total
                            } else {
                                dayCostUnavailable = true
                            }
                        } else {
                            cost = amount
                        }
                    }
                }
            }
        }

        if dayCostUnavailable {
            cost = nil
        }

        return DayAggregationResult(tokens: tokens, cost: cost, requests: requests)
    }

    private static func aggregateMonth(ctx: DailyAggregationContext) -> DayAggregationResult {
        var tokens = 0
        var cost: Double?
        var requests = 0
        var monthCostUnavailable = false

        for date in ctx.allDates {
            guard let parsed = self.parseDate(date, calendar: ctx.calendar),
                  parsed >= ctx.startOfMonth,
                  parsed <= ctx.now
            else { continue }

            if let amounts = ctx.amountMap[date] {
                for items in amounts.values {
                    for item in items {
                        guard let category = DeepSeekUsageCategory(rawValue: item.type ?? "") else { continue }
                        if category == .request {
                            requests += Self.parseTokenAmount(item.amount)
                        } else {
                            tokens += Self.parseTokenAmount(item.amount)
                        }
                    }
                }
            }

            if let costs = ctx.costMap[date] {
                for items in costs.values {
                    for item in items {
                        guard let category = DeepSeekUsageCategory(rawValue: item.type ?? "") else {
                            monthCostUnavailable = true
                            continue
                        }
                        if category != .request {
                            guard let amount = Self.parseValidCostAmount(item.amount) else {
                                monthCostUnavailable = true
                                continue
                            }
                            if let existing = cost {
                                let total = existing + amount
                                if total.isFinite {
                                    cost = total
                                } else {
                                    monthCostUnavailable = true
                                }
                            } else {
                                cost = amount
                            }
                        }
                    }
                }
            }
        }

        if monthCostUnavailable {
            cost = nil
        }

        return DayAggregationResult(tokens: tokens, cost: cost, requests: requests)
    }

    private static func buildBreakdowns(
        totalAmounts: [DeepSeekModelUsage],
        totalCosts: [DeepSeekCostModelUsage]) -> (String?, [DeepSeekCategoryBreakdown], [DeepSeekModelCost])
    {
        var modelTokens: [String: Int] = [:]
        var categoryTokens: [DeepSeekUsageCategory: Int] = [:]
        var categoryCosts: [DeepSeekUsageCategory: Double] = [:]
        var invalidCategories: Set<DeepSeekUsageCategory> = []
        var modelCosts = ModelCostTotals()

        for modelUsage in totalAmounts {
            guard let model = modelUsage.model else { continue }
            var total = 0
            for item in modelUsage.usage ?? [] {
                guard let category = DeepSeekUsageCategory(rawValue: item.type ?? "") else { continue }
                if category != .request {
                    let amount = Self.parseTokenAmount(item.amount)
                    total += amount
                    categoryTokens[category, default: 0] += amount
                }
            }
            modelTokens[model] = total
        }

        for costUsage in totalCosts {
            guard let model = costUsage.model else { continue }
            guard let items = costUsage.usage else {
                modelCosts.add(nil, model: model)
                continue
            }
            for item in items {
                guard let category = DeepSeekUsageCategory(rawValue: item.type ?? "") else {
                    modelCosts.add(nil, model: model)
                    continue
                }
                if category != .request {
                    let amount = Self.parseValidCostAmount(item.amount)
                    if let amount {
                        if !invalidCategories.contains(category) {
                            let total = categoryCosts[category, default: 0] + amount
                            if total.isFinite {
                                categoryCosts[category] = total
                            } else {
                                invalidCategories.insert(category)
                                categoryCosts.removeValue(forKey: category)
                            }
                        }
                    } else {
                        invalidCategories.insert(category)
                        categoryCosts.removeValue(forKey: category)
                    }
                    modelCosts.add(amount, model: model)
                }
            }
        }

        let topModel = modelTokens.max {
            if $0.value == $1.value {
                return $0.key > $1.key
            }
            return $0.value < $1.value
        }?.key

        var breakdown: [DeepSeekCategoryBreakdown] = []
        for category in [DeepSeekUsageCategory.promptCacheHitToken, .promptCacheMissToken, .responseToken] {
            let catCost = invalidCategories.contains(category) ? nil : categoryCosts[category]
            breakdown.append(DeepSeekCategoryBreakdown(
                category: category,
                tokens: categoryTokens[category] ?? 0,
                cost: catCost))
        }

        return (topModel, breakdown, modelCosts.values)
    }

    private struct ModelTokenCounts {
        var tokens = 0
        var requests = 0
        var cacheHit = 0
        var cacheMiss = 0
        var response = 0
    }

    private static func parseModelTokens(items: [DeepSeekUsageItem]?) -> ModelTokenCounts {
        guard let items else { return ModelTokenCounts() }
        var counts = ModelTokenCounts()
        for item in items {
            guard let category = DeepSeekUsageCategory(rawValue: item.type ?? "") else { continue }
            let amount = Self.parseTokenAmount(item.amount)
            switch category {
            case .request:
                counts.requests += amount
            case .promptCacheHitToken:
                counts.cacheHit += amount
                counts.tokens += amount
            case .promptCacheMissToken:
                counts.cacheMiss += amount
                counts.tokens += amount
            case .responseToken:
                counts.response += amount
                counts.tokens += amount
            }
        }
        return counts
    }

    private static func parseModelCost(items: [DeepSeekCostItem]?) -> (cost: Double?, isInvalid: Bool) {
        guard let items else { return (nil, false) }
        var sum: Double = 0
        var hasValidCost = false
        for item in items {
            guard let category = DeepSeekUsageCategory(rawValue: item.type ?? "") else {
                return (nil, true)
            }
            if category != .request {
                guard let amount = Self.parseValidCostAmount(item.amount) else {
                    return (nil, true)
                }
                sum += amount
                hasValidCost = true
            }
        }
        if hasValidCost, !sum.isFinite {
            return (nil, true)
        }
        return (hasValidCost ? sum : nil, false)
    }

    private static func buildDailyUsages(ctx: DailyAggregationContext) -> [DeepSeekDailyUsage] {
        var result: [DeepSeekDailyUsage] = []

        for date in ctx.allDates.sorted() {
            guard let parsed = self.parseDate(date, calendar: ctx.calendar),
                  parsed >= ctx.startOfMonth,
                  parsed <= ctx.now
            else { continue }

            let amounts = ctx.amountMap[date] ?? [:]
            let costs = ctx.costMap[date] ?? [:]

            var dayTokens = 0
            var dayCost: Double?
            var dayRequests = 0
            var dayCacheHits = 0
            var dayCacheMisses = 0
            var dayResponses = 0
            var dayCostUnavailable = false

            var modelBreakdowns: [CostUsageDailyReport.ModelBreakdown] = []
            let allModels = Set(amounts.keys).union(costs.keys).sorted()

            for model in allModels {
                let counts = Self.parseModelTokens(items: amounts[model])
                let (mCost, isCostInvalid) = Self.parseModelCost(items: costs[model])

                if isCostInvalid {
                    dayCostUnavailable = true
                }

                dayTokens += counts.tokens
                dayRequests += counts.requests
                dayCacheHits += counts.cacheHit
                dayCacheMisses += counts.cacheMiss
                dayResponses += counts.response

                if let mCost {
                    if !dayCostUnavailable {
                        if let existing = dayCost {
                            let total = existing + mCost
                            if total.isFinite {
                                dayCost = total
                            } else {
                                dayCostUnavailable = true
                            }
                        } else {
                            dayCost = mCost
                        }
                    }
                }

                if counts.tokens > 0 || counts.requests > 0 || (mCost ?? 0) > 0 {
                    modelBreakdowns.append(CostUsageDailyReport.ModelBreakdown(
                        modelName: model,
                        costUSD: mCost,
                        totalTokens: counts.tokens,
                        requestCount: counts.requests,
                        inputTokens: counts.cacheMiss,
                        outputTokens: counts.response,
                        cacheReadTokens: counts.cacheHit))
                }
            }

            if dayCostUnavailable {
                dayCost = nil
            }

            let hasCategoryTokens = dayCacheHits > 0 || dayCacheMisses > 0 || dayResponses > 0
            result.append(DeepSeekDailyUsage(
                date: date,
                totalTokens: dayTokens,
                cost: dayCost,
                requestCount: dayRequests,
                inputTokens: hasCategoryTokens ? dayCacheMisses : nil,
                outputTokens: hasCategoryTokens ? dayResponses : nil,
                cacheReadTokens: hasCategoryTokens ? dayCacheHits : nil,
                modelBreakdowns: modelBreakdowns.isEmpty ? nil : modelBreakdowns))
        }

        return result
    }

    // MARK: - Helpers

    fileprivate static func parseTokenAmount(_ value: String?) -> Int {
        guard let value, let intValue = Int64(value.trimmingCharacters(in: .whitespacesAndNewlines)) else {
            return 0
        }
        return Int(intValue)
    }

    fileprivate static func parseValidCostAmount(_ value: String?) -> Double? {
        guard let value else { return nil }
        let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty,
              let amount = Double(trimmed),
              amount.isFinite,
              amount.sign != .minus
        else { return nil }
        return amount
    }

    fileprivate static func parseDate(_ text: String, calendar: Calendar) -> Date? {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return nil }
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.calendar = calendar
        formatter.timeZone = calendar.timeZone
        formatter.dateFormat = "yyyy-MM-dd"
        return formatter.date(from: trimmed)
    }
}

// MARK: - By-API-key dashboard series

extension DeepSeekUsageCostParser {
    /// The Platform usage page lists spend per key and model as timestamped
    /// buckets. Fold that into the same summary the menu already shows.
    static func parseByAPIKey(
        amountData: Data,
        costData: Data,
        now: Date = Date(),
        calendar: Calendar = .current,
        rangeStart: Date? = nil,
        rangeEnd: Date? = nil) throws -> DeepSeekUsageSummary
    {
        let (amount, cost) = try self.decodeByAPIKeyPayloads(amountData: amountData, costData: costData)

        let start = rangeStart ?? calendar.date(byAdding: .day, value: -29, to: calendar.startOfDay(for: now)) ?? now
        let end = rangeEnd ?? calendar.date(byAdding: .day, value: 1, to: calendar.startOfDay(for: now)) ?? now
        let startSeconds = Int(start.timeIntervalSince1970)
        let endSeconds = Int(end.timeIntervalSince1970)

        let parsedAmount = self.parseAmountSeries(
            amount.data?.bizData?.series ?? [],
            startSeconds: startSeconds,
            endSeconds: endSeconds,
            calendar: calendar)

        let parsedCost = self.parseCostSeries(
            cost.data?.bizData?.data ?? [],
            startSeconds: startSeconds,
            endSeconds: endSeconds,
            calendar: calendar)

        let apiKeyIDs = parsedAmount.apiKeyIDs.union(parsedCost.apiKeyIDs)
        let dailyResult = self.buildByAPIKeyDailyUsages(
            dayAmounts: parsedAmount.dayAmounts,
            dayCosts: parsedCost.dayCosts,
            dayModelCosts: parsedCost.dayModelCosts,
            dayCostReported: parsedCost.dayCostReported)

        let todayString = AggregationContext.dayString(now, calendar: calendar)
        let breakdown = [
            DeepSeekUsageCategory.promptCacheHitToken,
            .promptCacheMissToken,
            .responseToken,
        ].map { category in
            DeepSeekCategoryBreakdown(category: category, tokens: parsedAmount.categoryTotals[category] ?? 0, cost: nil)
        }
        let topModel = parsedAmount.modelTokens.max {
            if $0.value == $1.value {
                return $0.key > $1.key
            }
            return $0.value < $1.value
        }?.key

        let today = dailyResult.daily.first { $0.date == todayString }
        return DeepSeekUsageSummary(
            todayTokens: today?.totalTokens ?? 0,
            currentMonthTokens: dailyResult.periodTokens,
            todayCost: parsedCost.dayCosts[todayString],
            currentMonthCost: parsedCost.periodCost,
            requestCount: today?.requestCount ?? 0,
            currentMonthRequestCount: dailyResult.periodRequests,
            topModel: topModel,
            categoryBreakdown: breakdown,
            daily: dailyResult.daily,
            currency: parsedCost.currency,
            modelCosts: parsedCost.modelCosts,
            apiKeyCount: apiKeyIDs.count,
            period: .last30Days,
            updatedAt: now)
    }

    private struct ParsedAmountSeries {
        var dayAmounts: [String: [String: [DeepSeekUsageItem]]] = [:]
        var categoryTotals: [DeepSeekUsageCategory: Int] = [:]
        var modelTokens: [String: Int] = [:]
        var apiKeyIDs: Set<String> = []
    }

    private static func parseAmountSeries(
        _ seriesList: [ByAPIKeyAmountSeries],
        startSeconds: Int,
        endSeconds: Int,
        calendar: Calendar) -> ParsedAmountSeries
    {
        var result = ParsedAmountSeries()
        for series in seriesList {
            if let id = series.apiKey?.id {
                result.apiKeyIDs.insert(id)
            }
            let model = series.model ?? "unknown"
            for bucket in series.buckets ?? [] where bucket.time >= startSeconds && bucket.time < endSeconds {
                let date = self.dayString(fromUnix: bucket.time, calendar: calendar)
                var items: [DeepSeekUsageItem] = []
                for (type, scalar) in bucket.usage ?? [:] {
                    items.append(DeepSeekUsageItem(type: type, amount: scalar.value))
                    guard let category = DeepSeekUsageCategory(rawValue: type) else { continue }
                    let value = self.parseTokenAmount(scalar.value)
                    if category == .request {
                        continue
                    }
                    result.categoryTotals[category, default: 0] += value
                    result.modelTokens[model, default: 0] += value
                }
                var models = result.dayAmounts[date] ?? [:]
                var merged = models[model] ?? []
                merged.append(contentsOf: items)
                models[model] = merged
                result.dayAmounts[date] = models
            }
        }
        return result
    }

    private struct ParsedCostSeries {
        var dayCosts: [String: Double] = [:]
        var dayModelCosts: [String: [String: Double]] = [:]
        var dayCostReported: Set<String> = []
        var modelCosts: [DeepSeekModelCost] = []
        var apiKeyIDs: Set<String> = []
        var currency: String = "CNY"
        var periodCost: Double?
    }

    private static func parseCostSeries(
        _ costBlocks: [ByAPIKeyCostCurrency],
        startSeconds: Int,
        endSeconds: Int,
        calendar: Calendar) -> ParsedCostSeries
    {
        var result = ParsedCostSeries()
        let selectedBlock = self.preferredCostBlock(costBlocks)
        result.currency = selectedBlock?.currency?.trimmingCharacters(in: .whitespacesAndNewlines).nilIfEmpty ?? "CNY"
        var modelCostTotals = ModelCostTotals()
        var dayModelCostTotals: [String: ModelCostTotals] = [:]
        var dayCostTotals: [String: Double] = [:]
        var dayCostUnavailable: Set<String> = []
        var periodCostUnavailable = false

        for series in selectedBlock?.series ?? [] {
            if let id = series.apiKey?.id {
                result.apiKeyIDs.insert(id)
            }
            guard let buckets = series.buckets else {
                modelCostTotals.add(nil, model: series.model)
                periodCostUnavailable = true
                continue
            }
            let rawModel = series.model?.trimmingCharacters(in: .whitespacesAndNewlines)
            for bucket in buckets where bucket.time >= startSeconds && bucket.time < endSeconds {
                let date = self.dayString(fromUnix: bucket.time, calendar: calendar)
                result.dayCostReported.insert(date)
                let amount = bucket.cost.flatMap { Self.parseValidCostAmount($0.value) }
                if !dayCostUnavailable.contains(date) {
                    if let amount {
                        let total = (dayCostTotals[date] ?? 0) + amount
                        if total.isFinite {
                            dayCostTotals[date] = total
                        } else {
                            dayCostUnavailable.insert(date)
                            dayCostTotals.removeValue(forKey: date)
                            periodCostUnavailable = true
                        }
                    } else {
                        dayCostUnavailable.insert(date)
                        dayCostTotals.removeValue(forKey: date)
                        periodCostUnavailable = true
                    }
                } else if amount == nil {
                    periodCostUnavailable = true
                }
                if let rawModel, !rawModel.isEmpty {
                    dayModelCostTotals[date, default: ModelCostTotals()].add(amount, model: rawModel)
                }
                modelCostTotals.add(amount, model: series.model)
            }
        }
        result.dayCosts = dayCostTotals
        result.dayModelCosts = dayModelCostTotals.mapValues(\.totalsDictionary)
        result.modelCosts = modelCostTotals.values

        if periodCostUnavailable || !dayCostUnavailable.isEmpty {
            result.periodCost = nil
        } else if result.dayCostReported.isEmpty {
            result.periodCost = nil
        } else {
            let total = dayCostTotals.values.reduce(0.0, +)
            result.periodCost = total.isFinite ? total : nil
        }
        return result
    }

    private struct ByAPIKeyDailyResult {
        let daily: [DeepSeekDailyUsage]
        let periodTokens: Int
        let periodRequests: Int
    }

    private static func buildByAPIKeyDailyUsages(
        dayAmounts: [String: [String: [DeepSeekUsageItem]]],
        dayCosts: [String: Double],
        dayModelCosts: [String: [String: Double]],
        dayCostReported: Set<String>) -> ByAPIKeyDailyResult
    {
        var periodTokens = 0
        var periodRequests = 0
        var daily: [DeepSeekDailyUsage] = []

        for date in Set(dayAmounts.keys).union(dayCosts.keys).sorted() {
            var tokens = 0
            var requests = 0
            var cacheHits = 0
            var cacheMisses = 0
            var responses = 0
            var modelBreakdowns: [CostUsageDailyReport.ModelBreakdown] = []

            let modelAmounts = dayAmounts[date] ?? [:]
            let modelCostsOnDate = dayModelCosts[date] ?? [:]
            let allModels = Set(modelAmounts.keys).union(modelCostsOnDate.keys).sorted()

            for model in allModels {
                var mTokens = 0
                var mRequests = 0
                var mCacheHit = 0
                var mCacheMiss = 0
                var mResponse = 0

                if let items = modelAmounts[model] {
                    for item in items {
                        guard let category = DeepSeekUsageCategory(rawValue: item.type ?? "") else { continue }
                        let amount = self.parseTokenAmount(item.amount)
                        switch category {
                        case .request:
                            mRequests += amount
                        case .promptCacheHitToken:
                            mCacheHit += amount
                            mTokens += amount
                        case .promptCacheMissToken:
                            mCacheMiss += amount
                            mTokens += amount
                        case .responseToken:
                            mResponse += amount
                            mTokens += amount
                        }
                    }
                }

                let mCost: Double? = modelCostsOnDate[model]

                requests += mRequests
                tokens += mTokens
                cacheHits += mCacheHit
                cacheMisses += mCacheMiss
                responses += mResponse

                if mTokens > 0 || mRequests > 0 || (mCost ?? 0) > 0 {
                    modelBreakdowns.append(CostUsageDailyReport.ModelBreakdown(
                        modelName: model,
                        costUSD: mCost,
                        totalTokens: mTokens,
                        requestCount: mRequests,
                        inputTokens: mCacheMiss,
                        outputTokens: mResponse,
                        cacheReadTokens: mCacheHit))
                }
            }

            periodTokens += tokens
            periodRequests += requests
            let hasCategoryTokens = cacheHits > 0 || cacheMisses > 0 || responses > 0
            daily.append(DeepSeekDailyUsage(
                date: date,
                totalTokens: tokens,
                cost: dayCosts[date],
                requestCount: requests,
                inputTokens: hasCategoryTokens ? cacheMisses : nil,
                outputTokens: hasCategoryTokens ? responses : nil,
                cacheReadTokens: hasCategoryTokens ? cacheHits : nil,
                modelBreakdowns: modelBreakdowns.isEmpty ? nil : modelBreakdowns))
        }

        return ByAPIKeyDailyResult(
            daily: daily,
            periodTokens: periodTokens,
            periodRequests: periodRequests)
    }

    private static func decodeByAPIKeyPayloads(amountData: Data, costData: Data) throws
        -> (ByAPIKeyAmountPayload, ByAPIKeyCostPayload)
    {
        let amount: ByAPIKeyAmountPayload
        let cost: ByAPIKeyCostPayload
        do {
            amount = try JSONDecoder().decode(ByAPIKeyAmountPayload.self, from: amountData)
        } catch {
            throw DeepSeekUsageError.parseFailed("amount: \(self.decodingFailureDescription(error))")
        }
        do {
            cost = try JSONDecoder().decode(ByAPIKeyCostPayload.self, from: costData)
        } catch {
            throw DeepSeekUsageError.parseFailed("cost: \(self.decodingFailureDescription(error))")
        }
        if let code = amount.code, code != 0 {
            if self.isAuthenticationError(code) {
                throw DeepSeekUsageError.invalidPlatformToken
            }
            throw DeepSeekUsageError.apiError("amount code \(code)")
        }
        if let code = cost.code, code != 0 {
            if self.isAuthenticationError(code) {
                throw DeepSeekUsageError.invalidPlatformToken
            }
            throw DeepSeekUsageError.apiError("cost code \(code)")
        }
        if let bizCode = amount.data?.bizCode, bizCode != 0 {
            if self.isAuthenticationError(bizCode) {
                throw DeepSeekUsageError.invalidPlatformToken
            }
            throw DeepSeekUsageError.apiError("amount biz_code \(bizCode)")
        }
        if let bizCode = cost.data?.bizCode, bizCode != 0 {
            if self.isAuthenticationError(bizCode) {
                throw DeepSeekUsageError.invalidPlatformToken
            }
            throw DeepSeekUsageError.apiError("cost biz_code \(bizCode)")
        }
        guard amount.data?.bizData != nil else {
            throw DeepSeekUsageError.parseFailed("Missing amount biz_data")
        }
        guard cost.data?.bizData != nil else {
            throw DeepSeekUsageError.parseFailed("Missing cost biz_data")
        }

        return (amount, cost)
    }

    private static func preferredCostBlock(_ blocks: [ByAPIKeyCostCurrency]) -> ByAPIKeyCostCurrency? {
        func spend(_ block: ByAPIKeyCostCurrency) -> Double {
            (block.series ?? []).reduce(0) { total, series in
                total + (series.buckets ?? []).reduce(0) { $0 + ($1.cost.flatMap { Double($0.value) } ?? 0) }
            }
        }
        return blocks.first { $0.currency == "USD" && spend($0) > 0 }
            ?? blocks.first { spend($0) > 0 }
            ?? blocks.first { $0.currency == "USD" }
            ?? blocks.first
    }

    private static func dayString(fromUnix time: Int, calendar: Calendar) -> String {
        self.AggregationContext.dayString(Date(timeIntervalSince1970: TimeInterval(time)), calendar: calendar)
    }
}

private struct ModelCostTotals {
    private var totals: [String: Double] = [:]
    private var unavailable: Set<String> = []

    mutating func add(_ amount: Double?, model rawModel: String?) {
        guard let model = rawModel?.trimmingCharacters(in: .whitespacesAndNewlines),
              !model.isEmpty, !self.unavailable.contains(model)
        else { return }
        if let amount, amount.isFinite, amount.sign != .minus {
            let total = (self.totals[model] ?? 0) + amount
            if total.isFinite {
                self.totals[model] = total
                return
            }
        }
        // A malformed component must not leave a plausible but incomplete model total.
        self.unavailable.insert(model)
        self.totals.removeValue(forKey: model)
    }

    var values: [DeepSeekModelCost] {
        self.totals.map { DeepSeekModelCost(model: $0.key, cost: $0.value) }.sorted {
            $0.cost == $1.cost ? $0.model < $1.model : $0.cost > $1.cost
        }
    }

    var totalsDictionary: [String: Double] {
        self.totals
    }
}

extension String {
    fileprivate var nilIfEmpty: String? {
        self.isEmpty ? nil : self
    }
}

private struct ByAPIKeyAmountPayload: Decodable {
    let code: Int?
    let data: ByAPIKeyAmountData?
}

private struct ByAPIKeyAmountData: Decodable {
    let bizCode: Int?
    let bizData: ByAPIKeyAmountBiz?
    enum CodingKeys: String, CodingKey { case bizCode = "biz_code"; case bizData = "biz_data" }
}

private struct ByAPIKeyAmountBiz: Decodable {
    let series: [ByAPIKeyAmountSeries]?
}

private struct ByAPIKeyAmountSeries: Decodable {
    let apiKey: ByAPIKeyIdentity?
    let model: String?
    let buckets: [ByAPIKeyAmountBucket]?
    enum CodingKeys: String, CodingKey { case apiKey = "api_key"; case model, buckets }
}

private struct ByAPIKeyAmountBucket: Decodable {
    let time: Int
    let usage: [String: ByAPIKeyScalar]?
}

private struct ByAPIKeyCostPayload: Decodable {
    let code: Int?
    let data: ByAPIKeyCostData?
}

private struct ByAPIKeyCostData: Decodable {
    let bizCode: Int?
    let bizData: ByAPIKeyCostBiz?
    enum CodingKeys: String, CodingKey { case bizCode = "biz_code"; case bizData = "biz_data" }
}

private struct ByAPIKeyCostBiz: Decodable {
    let data: [ByAPIKeyCostCurrency]?
}

private struct ByAPIKeyCostCurrency: Decodable {
    let currency: String?
    let series: [ByAPIKeyCostSeries]?
}

private struct ByAPIKeyCostSeries: Decodable {
    let apiKey: ByAPIKeyIdentity?
    let model: String?
    let buckets: [ByAPIKeyCostBucket]?
    enum CodingKeys: String, CodingKey { case apiKey = "api_key"; case model, buckets }
}

private struct ByAPIKeyCostBucket: Decodable {
    let time: Int
    let cost: ByAPIKeyScalar?
}

private struct ByAPIKeyIdentity: Decodable {
    let name: String?
    let trackingID: String?
    enum CodingKeys: String, CodingKey { case name; case trackingID = "tracking_id" }

    init(from decoder: Decoder) throws {
        if let trackingID = try? decoder.singleValueContainer().decode(String.self) {
            self.trackingID = trackingID
            self.name = nil
        } else {
            let container = try decoder.container(keyedBy: CodingKeys.self)
            self.trackingID = try container.decodeIfPresent(String.self, forKey: .trackingID)
            self.name = try container.decodeIfPresent(String.self, forKey: .name)
        }
    }

    var id: String {
        let tracking = self.trackingID?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        if !tracking.isEmpty {
            return tracking
        }
        let name = self.name?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        return name.isEmpty ? "unknown" : name
    }
}

private struct ByAPIKeyScalar: Decodable {
    let value: String
    init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        if container.decodeNil() {
            self.value = "0"
        } else if let string = try? container.decode(String.self) {
            self.value = string
        } else if let int = try? container.decode(Int.self) {
            self.value = String(int)
        } else if let double = try? container.decode(Double.self) {
            self.value = String(double)
        } else {
            throw DecodingError.typeMismatch(
                String.self,
                .init(codingPath: decoder.codingPath, debugDescription: "Expected a scalar usage value"))
        }
    }
}
