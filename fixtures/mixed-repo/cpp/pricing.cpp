#include "pricing.hpp"
#include <algorithm>

namespace pricing {

void RuleSet::add(const std::string& name) {
    (void)name;
}

double RuleSet::apply(double amount) const {
    return amount;
}

PricingEngine::PricingEngine() = default;

double PricingEngine::calculate(double base, const std::vector<std::string>& tags) const {
    double total = apply(base);
    if (std::find(tags.begin(), tags.end(), "vip") != tags.end()) {
        total *= 0.9;
    }
    return total;
}

}  // namespace pricing
