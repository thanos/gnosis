#pragma once
#include <string>
#include <vector>

namespace pricing {

enum class RuleKind { Fixed, Percent };

class RuleSet {
public:
    void add(const std::string& name);
    double apply(double amount) const;
};

class PricingEngine : public RuleSet {
public:
    PricingEngine();
    double calculate(double base, const std::vector<std::string>& tags) const;
};

}  // namespace pricing
